use std::collections::HashMap;

use bevy::prelude::{Mat3, Vec3};
use cryspglib::spg_database::{SPACEGROUP_TYPES, get_spacegroup_operations};

use crate::{
    elements::normalize_symbol,
    model::{AtomSite, CrystalStructure, SiteId, SourceFormat, UnitCell},
};

use super::{ParseError, parse_number};

#[derive(Debug)]
struct LoopData {
    tags: Vec<String>,
    values: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct SymmetryOperation {
    matrix: Mat3,
    translation: Vec3,
    time_reversal: f32,
}

impl SymmetryOperation {
    fn identity() -> Self {
        Self {
            matrix: Mat3::IDENTITY,
            translation: Vec3::ZERO,
            time_reversal: 1.0,
        }
    }

    fn apply(self, p: Vec3) -> Vec3 {
        wrap01(self.matrix * p + self.translation)
    }
}

pub fn parse(text: &str) -> Result<CrystalStructure, ParseError> {
    let tokens = tokenize(text);
    let (scalars, loops, data_name) = parse_tokens(&tokens);
    let is_magnetic = loops.iter().any(|loop_data| {
        loop_data.tags.iter().any(|tag| {
            tag.starts_with("_atom_site_moment") || tag.starts_with("_space_group_symop_magn")
        })
    }) || scalars.keys().any(|tag| {
        tag.starts_with("_atom_site_moment") || tag.starts_with("_space_group_symop_magn")
    });

    let a = scalar_number(&scalars, "_cell_length_a")?;
    let b = scalar_number(&scalars, "_cell_length_b")?;
    let c = scalar_number(&scalars, "_cell_length_c")?;
    let alpha = optional_scalar_number(&scalars, "_cell_angle_alpha").unwrap_or(90.0);
    let beta = optional_scalar_number(&scalars, "_cell_angle_beta").unwrap_or(90.0);
    let gamma = optional_scalar_number(&scalars, "_cell_angle_gamma").unwrap_or(90.0);
    let cell = UnitCell::new(a, b, c, alpha, beta, gamma);

    let atom_loop = loops
        .iter()
        .find(|item| {
            item.tags.iter().any(|tag| tag == "_atom_site_fract_x")
                && item.tags.iter().any(|tag| tag == "_atom_site_fract_y")
                && item.tags.iter().any(|tag| tag == "_atom_site_fract_z")
        })
        .ok_or(ParseError::NoAtoms)?;

    let index = |names: &[&str]| -> Option<usize> {
        names
            .iter()
            .find_map(|name| atom_loop.tags.iter().position(|tag| tag == name))
    };
    let x_index = index(&["_atom_site_fract_x"]).unwrap();
    let y_index = index(&["_atom_site_fract_y"]).unwrap();
    let z_index = index(&["_atom_site_fract_z"]).unwrap();
    let symbol_index = index(&["_atom_site_type_symbol"]);
    let label_index = index(&["_atom_site_label"]);
    let occupancy_index = index(&["_atom_site_occupancy"]);

    let mut warnings = Vec::new();
    let magnetic_moments = magnetic_moments(&loops, &cell, &mut warnings);
    let mut base_atoms = Vec::new();
    for row in atom_loop.values.chunks(atom_loop.tags.len()) {
        if row.len() != atom_loop.tags.len() {
            continue;
        }
        let label = label_index
            .and_then(|i| row.get(i))
            .cloned()
            .unwrap_or_else(|| format!("A{}", base_atoms.len() + 1));
        let raw_symbol = symbol_index
            .and_then(|i| row.get(i))
            .map(String::as_str)
            .unwrap_or(&label);
        let fractional = Vec3::new(
            parse_number(&row[x_index], "_atom_site_fract_x")?,
            parse_number(&row[y_index], "_atom_site_fract_y")?,
            parse_number(&row[z_index], "_atom_site_fract_z")?,
        );
        let occupancy = occupancy_index
            .and_then(|i| row.get(i))
            .and_then(|value| parse_number(value, "_atom_site_occupancy").ok())
            .unwrap_or(1.0);
        let magnetic_moment = magnetic_moments.get(&label).copied();
        base_atoms.push(AtomSite {
            site_id: SiteId(base_atoms.len() as u64),
            element: normalize_symbol(raw_symbol),
            label,
            fractional: wrap01(fractional),
            occupancy,
            symmetry_rotation: Mat3::IDENTITY,
            symmetry_time_reversal: 1.0,
            magnetic_moment,
        });
    }

    if base_atoms.is_empty() {
        return Err(ParseError::NoAtoms);
    }

    let space_group = space_group_label(&scalars);
    let operations = symmetry_operations(&loops, &scalars, space_group.as_deref(), &mut warnings);
    let mut atoms = Vec::new();
    for atom in &base_atoms {
        for operation in &operations {
            let fractional = operation.apply(atom.fractional);
            let duplicate = atoms.iter().any(|existing: &AtomSite| {
                existing.element == atom.element
                    && periodic_distance(existing.fractional, fractional) < 1.0e-4
            });
            if !duplicate {
                let mut expanded = atom.clone();
                expanded.fractional = fractional;
                expanded.symmetry_rotation = operation.matrix;
                expanded.symmetry_time_reversal = operation.time_reversal;
                expanded.magnetic_moment = atom.magnetic_moment.map(|moment| {
                    operation.time_reversal
                        * operation.matrix.determinant().signum()
                        * cell.symmetry_to_cartesian(operation.matrix)
                        * moment
                });
                atoms.push(expanded);
            }
        }
    }

    let title = scalars
        .get("_chemical_name_common")
        .or_else(|| scalars.get("_chemical_formula_sum"))
        .cloned()
        .unwrap_or_else(|| {
            if data_name.is_empty() {
                "Untitled structure".to_string()
            } else {
                data_name.replace('_', " ")
            }
        });

    Ok(CrystalStructure {
        title,
        cell,
        asymmetric_atoms: base_atoms,
        atoms,
        space_group,
        symmetry_operation_count: operations.len(),
        source_format: if is_magnetic {
            SourceFormat::Mcif
        } else {
            SourceFormat::Cif
        },
        warnings,
        imported_visuals: Default::default(),
    })
}

fn space_group_label(scalars: &HashMap<String, String>) -> Option<String> {
    const SYMBOL_TAGS: [&str; 4] = [
        "_space_group_name_h-m_alt",
        "_space_group_name_h-m_ref",
        "_symmetry_space_group_name_h-m",
        "_space_group.name_h-m_alt",
    ];
    for tag in SYMBOL_TAGS {
        if let Some(value) = scalars.get(tag) {
            return Some(value.trim_matches(['\'', '"']).to_string());
        }
    }
    space_group_number(scalars).map(|number| format!("#{number}"))
}

fn space_group_number(scalars: &HashMap<String, String>) -> Option<usize> {
    const NUMBER_TAGS: [&str; 3] = [
        "_space_group_it_number",
        "_symmetry_int_tables_number",
        "_space_group.it_number",
    ];
    NUMBER_TAGS.iter().find_map(|tag| {
        scalars
            .get(*tag)
            .and_then(|value| value.trim().parse::<usize>().ok())
    })
}

fn scalar_number(scalars: &HashMap<String, String>, tag: &'static str) -> Result<f32, ParseError> {
    let value = scalars.get(tag).ok_or(ParseError::MissingField(tag))?;
    parse_number(value, tag)
}

fn optional_scalar_number(scalars: &HashMap<String, String>, tag: &'static str) -> Option<f32> {
    scalars
        .get(tag)
        .and_then(|value| parse_number(value, tag).ok())
}

fn magnetic_moments(
    loops: &[LoopData],
    cell: &UnitCell,
    warnings: &mut Vec<String>,
) -> HashMap<String, Vec3> {
    const MOMENT_LABELS: [&str; 3] = [
        "_atom_site_moment.label",
        "_atom_site_moment_label",
        "_atom_site.moment_label",
    ];
    const ATOM_LABELS: [&str; 2] = ["_atom_site_label", "_atom_site.label"];
    const CRYSTAL_X: [&str; 3] = [
        "_atom_site_moment.crystalaxis_x",
        "_atom_site_moment_crystalaxis_x",
        "_atom_site.moment_crystalaxis_x",
    ];
    const CRYSTAL_Y: [&str; 3] = [
        "_atom_site_moment.crystalaxis_y",
        "_atom_site_moment_crystalaxis_y",
        "_atom_site.moment_crystalaxis_y",
    ];
    const CRYSTAL_Z: [&str; 3] = [
        "_atom_site_moment.crystalaxis_z",
        "_atom_site_moment_crystalaxis_z",
        "_atom_site.moment_crystalaxis_z",
    ];
    const CARTESIAN_X: [&str; 3] = [
        "_atom_site_moment.cartn_x",
        "_atom_site_moment_cartn_x",
        "_atom_site.moment_cartn_x",
    ];
    const CARTESIAN_Y: [&str; 3] = [
        "_atom_site_moment.cartn_y",
        "_atom_site_moment_cartn_y",
        "_atom_site.moment_cartn_y",
    ];
    const CARTESIAN_Z: [&str; 3] = [
        "_atom_site_moment.cartn_z",
        "_atom_site_moment_cartn_z",
        "_atom_site.moment_cartn_z",
    ];

    let mut moments = HashMap::new();
    for loop_data in loops {
        let index = |names: &[&str]| {
            names
                .iter()
                .find_map(|name| loop_data.tags.iter().position(|tag| tag == name))
        };
        let label_index = index(&MOMENT_LABELS).or_else(|| index(&ATOM_LABELS));
        let crystal_indices = (index(&CRYSTAL_X), index(&CRYSTAL_Y), index(&CRYSTAL_Z));
        let cartesian_indices = (
            index(&CARTESIAN_X),
            index(&CARTESIAN_Y),
            index(&CARTESIAN_Z),
        );
        let (coordinate_kind, component_indices) = if all_present(crystal_indices) {
            ("crystal-axis", crystal_indices)
        } else if all_present(cartesian_indices) {
            ("Cartesian", cartesian_indices)
        } else {
            continue;
        };
        let Some(label_index) = label_index else {
            warnings.push(
                "Ignored magnetic moments without _atom_site_moment.label or _atom_site_label."
                    .to_string(),
            );
            continue;
        };
        for row in loop_data.values.chunks(loop_data.tags.len()) {
            let Some(label) = row.get(label_index) else {
                continue;
            };
            let (Some(x_index), Some(y_index), Some(z_index)) = component_indices else {
                unreachable!("component indices were checked above");
            };
            let components = (
                row.get(x_index)
                    .and_then(|value| parse_number(value, "magnetic moment").ok()),
                row.get(y_index)
                    .and_then(|value| parse_number(value, "magnetic moment").ok()),
                row.get(z_index)
                    .and_then(|value| parse_number(value, "magnetic moment").ok()),
            );
            let (Some(x), Some(y), Some(z)) = components else {
                warnings.push(format!(
                    "Ignored incomplete magnetic moment for site '{label}'."
                ));
                continue;
            };
            let components = Vec3::new(x, y, z);
            let moment = if coordinate_kind == "crystal-axis" {
                cell.crystal_axes_to_cartesian(components)
            } else {
                cell.magcif_cartesian_to_cartesian(components)
            };
            moments.insert(label.clone(), moment);
        }
    }
    if !moments.is_empty() {
        warnings.push(format!(
            "Loaded {} magnetic moment(s) from mCIF.",
            moments.len()
        ));
    }
    moments
}

fn all_present(indices: (Option<usize>, Option<usize>, Option<usize>)) -> bool {
    indices.0.is_some() && indices.1.is_some() && indices.2.is_some()
}

fn symmetry_operations(
    loops: &[LoopData],
    scalars: &HashMap<String, String>,
    space_group_symbol: Option<&str>,
    warnings: &mut Vec<String>,
) -> Vec<SymmetryOperation> {
    if let Some(operations) = magnetic_symmetry_operations(loops, scalars, warnings) {
        return operations;
    }
    const TAGS: [&str; 3] = [
        "_space_group_symop_operation_xyz",
        "_symmetry_equiv_pos_as_xyz",
        "_space_group_symop.operation_xyz",
    ];
    let mut raw_operations = Vec::new();
    for loop_data in loops {
        if let Some(index) = TAGS
            .iter()
            .find_map(|tag| loop_data.tags.iter().position(|value| value == tag))
        {
            for row in loop_data.values.chunks(loop_data.tags.len()) {
                if let Some(value) = row.get(index) {
                    raw_operations.push(value.clone());
                }
            }
        }
    }
    for tag in TAGS {
        if let Some(value) = scalars.get(tag) {
            raw_operations.push(value.clone());
        }
    }
    if raw_operations.is_empty() {
        if let Some(operations) =
            symmetry_operations_from_database(space_group_number(scalars), space_group_symbol)
        {
            warnings.push(format!(
                "Generated {} symmetry operations from space group {}.",
                operations.len(),
                space_group_symbol.unwrap_or("metadata")
            ));
            return operations;
        }
        return vec![SymmetryOperation::identity()];
    }

    let parsed: Vec<_> = raw_operations
        .iter()
        .filter_map(|raw| match parse_symmetry_operation(raw) {
            Ok(value) => Some(value),
            Err(message) => {
                warnings.push(format!("Ignored symmetry operation '{raw}': {message}"));
                None
            }
        })
        .collect();
    if parsed.is_empty() {
        warnings.push("No symmetry operations could be parsed; using identity.".to_string());
        vec![SymmetryOperation::identity()]
    } else {
        parsed
    }
}

fn magnetic_symmetry_operations(
    loops: &[LoopData],
    scalars: &HashMap<String, String>,
    warnings: &mut Vec<String>,
) -> Option<Vec<SymmetryOperation>> {
    const OPERATION_TAGS: [&str; 4] = [
        "_space_group_symop_magn_operation.xyz",
        "_space_group_symop_magn_operation_xyz",
        "_space_group_symop.magn_operation.xyz",
        "_space_group_symop.magn_operation_xyz",
    ];
    const CENTERING_TAGS: [&str; 4] = [
        "_space_group_symop_magn_centering.xyz",
        "_space_group_symop_magn_centering_xyz",
        "_space_group_symop.magn_centering.xyz",
        "_space_group_symop.magn_centering_xyz",
    ];
    const TIME_REVERSAL_TAGS: [&str; 4] = [
        "_space_group_symop_magn_operation.timereversal",
        "_space_group_symop_magn_operation_timereversal",
        "_space_group_symop.magn_operation.timereversal",
        "_space_group_symop.magn_operation_timereversal",
    ];
    const CENTERING_TIME_REVERSAL_TAGS: [&str; 4] = [
        "_space_group_symop_magn_centering.timereversal",
        "_space_group_symop_magn_centering_timereversal",
        "_space_group_symop.magn_centering.timereversal",
        "_space_group_symop.magn_centering_timereversal",
    ];

    let operations = parse_magnetic_operation_rows(
        loops,
        scalars,
        &OPERATION_TAGS,
        &TIME_REVERSAL_TAGS,
        warnings,
    );
    if operations.is_empty() {
        return None;
    }
    let centerings = parse_magnetic_operation_rows(
        loops,
        scalars,
        &CENTERING_TAGS,
        &CENTERING_TIME_REVERSAL_TAGS,
        warnings,
    );
    let centerings = if centerings.is_empty() {
        vec![SymmetryOperation::identity()]
    } else {
        centerings
    };
    let mut combined = Vec::new();
    for operation in operations {
        for centering in &centerings {
            combined.push(SymmetryOperation {
                matrix: operation.matrix * centering.matrix,
                translation: operation.matrix * centering.translation + operation.translation,
                time_reversal: operation.time_reversal * centering.time_reversal,
            });
        }
    }
    warnings.push(format!(
        "Expanded magnetic symmetry with {} operation(s), including time reversal.",
        combined.len()
    ));
    Some(combined)
}

fn parse_magnetic_operation_rows(
    loops: &[LoopData],
    scalars: &HashMap<String, String>,
    operation_tags: &[&str],
    time_reversal_tags: &[&str],
    warnings: &mut Vec<String>,
) -> Vec<SymmetryOperation> {
    let mut raw_operations = Vec::new();
    for loop_data in loops {
        let operation_index = operation_tags
            .iter()
            .find_map(|tag| loop_data.tags.iter().position(|value| value == tag));
        let Some(operation_index) = operation_index else {
            continue;
        };
        let time_index = time_reversal_tags
            .iter()
            .find_map(|tag| loop_data.tags.iter().position(|value| value == tag));
        for row in loop_data.values.chunks(loop_data.tags.len()) {
            if let Some(raw) = row.get(operation_index) {
                raw_operations.push((
                    raw.clone(),
                    time_index.and_then(|index| row.get(index).cloned()),
                ));
            }
        }
    }
    for tag in operation_tags {
        if let Some(raw) = scalars.get(*tag) {
            raw_operations.push((raw.clone(), None));
        }
    }
    raw_operations
        .into_iter()
        .filter_map(|(raw, time_reversal)| {
            match parse_magnetic_symmetry_operation(&raw, time_reversal.as_deref()) {
                Ok(operation) => Some(operation),
                Err(message) => {
                    warnings.push(format!(
                        "Ignored magnetic symmetry operation '{raw}': {message}"
                    ));
                    None
                }
            }
        })
        .collect()
}

fn parse_magnetic_symmetry_operation(
    raw: &str,
    time_reversal_column: Option<&str>,
) -> Result<SymmetryOperation, String> {
    let cleaned = raw.trim().trim_matches(['\'', '"']).replace(' ', "");
    let mut parts: Vec<_> = cleaned.split(',').collect();
    let inline_time_reversal = if parts.len() == 4 {
        parse_time_reversal(parts.pop().expect("four parts"))?
    } else {
        1.0
    };
    if parts.len() != 3 {
        return Err("expected x,y,z with an optional +1 or -1 time-reversal suffix".to_string());
    }
    let spatial = parse_symmetry_operation(&parts.join(","))?;
    let time_reversal = time_reversal_column
        .map(parse_time_reversal)
        .transpose()?
        .unwrap_or(inline_time_reversal);
    Ok(SymmetryOperation {
        time_reversal,
        ..spatial
    })
}

fn parse_time_reversal(raw: &str) -> Result<f32, String> {
    match raw.trim().trim_matches(['\'', '"']) {
        "+1" | "1" => Ok(1.0),
        "-1" => Ok(-1.0),
        value => Err(format!("expected +1 or -1, got '{value}'")),
    }
}

fn symmetry_operations_from_database(
    number: Option<usize>,
    symbol: Option<&str>,
) -> Option<Vec<SymmetryOperation>> {
    if number.is_none() && symbol.is_none() {
        return None;
    }
    let normalized_symbol = symbol.map(normalize_space_group_symbol);
    let candidates: Vec<_> = SPACEGROUP_TYPES
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, value)| number.is_none_or(|number| value.number == number))
        .collect();
    let symbol_match = normalized_symbol.as_ref().and_then(|target| {
        candidates
            .iter()
            .find(|(_, value)| {
                [
                    value.international,
                    value.international_full,
                    value.international_short,
                ]
                .iter()
                .any(|candidate| normalize_space_group_symbol(candidate) == *target)
            })
            .map(|(index, _)| *index)
    });
    let hall_number = symbol_match.or_else(|| {
        number
            .is_some()
            .then(|| candidates.first().map(|(index, _)| *index))
            .flatten()
    })?;

    let symmetry = get_spacegroup_operations(hall_number)?;
    Some(
        symmetry
            .rot
            .iter()
            .zip(symmetry.trans.iter())
            .map(|(rotation, translation)| {
                let rows = rotation;
                SymmetryOperation {
                    matrix: Mat3::from_cols_array(&[
                        rows[0][0] as f32,
                        rows[1][0] as f32,
                        rows[2][0] as f32,
                        rows[0][1] as f32,
                        rows[1][1] as f32,
                        rows[2][1] as f32,
                        rows[0][2] as f32,
                        rows[1][2] as f32,
                        rows[2][2] as f32,
                    ]),
                    translation: Vec3::new(
                        translation[0] as f32,
                        translation[1] as f32,
                        translation[2] as f32,
                    ),
                    time_reversal: 1.0,
                }
            })
            .collect(),
    )
}

fn normalize_space_group_symbol(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '\'' && *c != '"')
        .flat_map(char::to_lowercase)
        .collect()
}

fn parse_symmetry_operation(raw: &str) -> Result<SymmetryOperation, String> {
    let cleaned = raw.trim().trim_matches(['\'', '"']).replace(' ', "");
    let parts: Vec<_> = cleaned.split(',').collect();
    if parts.len() != 3 {
        return Err("expected three comma-separated coordinates".to_string());
    }
    let mut rows = [[0.0; 3]; 3];
    let mut translation = [0.0; 3];
    for (row, expression) in parts.iter().enumerate() {
        let (coefficients, offset) = parse_linear_expression(expression)?;
        rows[row] = coefficients;
        translation[row] = offset;
    }
    let matrix = Mat3::from_cols_array(&[
        rows[0][0], rows[1][0], rows[2][0], rows[0][1], rows[1][1], rows[2][1], rows[0][2],
        rows[1][2], rows[2][2],
    ]);
    Ok(SymmetryOperation {
        matrix,
        translation: Vec3::from_array(translation),
        time_reversal: 1.0,
    })
}

fn parse_linear_expression(expression: &str) -> Result<([f32; 3], f32), String> {
    let mut normalized = expression.replace('-', "+-");
    if normalized.starts_with("+-") {
        normalized.remove(0);
    }
    let mut coefficients = [0.0; 3];
    let mut offset = 0.0;
    for term in normalized.split('+').filter(|value| !value.is_empty()) {
        let variable = term
            .char_indices()
            .find(|(_, c)| matches!(c, 'x' | 'y' | 'z'));
        if let Some((index, variable)) = variable {
            let coefficient_text = &term[..index];
            let coefficient = match coefficient_text {
                "" => 1.0,
                "-" => -1.0,
                other => parse_fraction(other)?,
            };
            let position = match variable {
                'x' => 0,
                'y' => 1,
                'z' => 2,
                _ => unreachable!(),
            };
            coefficients[position] += coefficient;
            let suffix = &term[index + 1..];
            if !suffix.is_empty() {
                return Err(format!("unsupported term '{term}'"));
            }
        } else {
            offset += parse_fraction(term)?;
        }
    }
    Ok((coefficients, offset))
}

fn parse_fraction(value: &str) -> Result<f32, String> {
    if let Some((a, b)) = value.split_once('/') {
        let numerator = a
            .parse::<f32>()
            .map_err(|_| format!("invalid fraction '{value}'"))?;
        let denominator = b
            .parse::<f32>()
            .map_err(|_| format!("invalid fraction '{value}'"))?;
        if denominator.abs() < f32::EPSILON {
            return Err("division by zero".to_string());
        }
        Ok(numerator / denominator)
    } else {
        value
            .parse::<f32>()
            .map_err(|_| format!("invalid number '{value}'"))
    }
}

fn wrap01(value: Vec3) -> Vec3 {
    Vec3::new(
        value.x.rem_euclid(1.0),
        value.y.rem_euclid(1.0),
        value.z.rem_euclid(1.0),
    )
}

fn periodic_distance(a: Vec3, b: Vec3) -> f32 {
    let delta = (a - b).abs();
    Vec3::new(
        delta.x.min(1.0 - delta.x),
        delta.y.min(1.0 - delta.y),
        delta.z.min(1.0 - delta.z),
    )
    .length()
}

fn parse_tokens(tokens: &[String]) -> (HashMap<String, String>, Vec<LoopData>, String) {
    let mut scalars = HashMap::new();
    let mut loops = Vec::new();
    let mut data_name = String::new();
    let mut i = 0;
    while i < tokens.len() {
        let lower = tokens[i].to_ascii_lowercase();
        if let Some(name) = lower.strip_prefix("data_") {
            if data_name.is_empty() {
                data_name = name.to_string();
            }
            i += 1;
        } else if lower == "loop_" {
            i += 1;
            let mut tags = Vec::new();
            while i < tokens.len() && tokens[i].starts_with('_') {
                tags.push(tokens[i].to_ascii_lowercase());
                i += 1;
            }
            let mut values = Vec::new();
            while i < tokens.len() && !is_control_token(&tokens[i]) {
                values.push(tokens[i].clone());
                i += 1;
            }
            if !tags.is_empty() {
                loops.push(LoopData { tags, values });
            }
        } else if lower.starts_with('_') {
            if let Some(value) = tokens.get(i + 1) {
                scalars.insert(lower, value.clone());
                i += 2;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    (scalars, loops, data_name)
}

fn is_control_token(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    lower == "loop_"
        || lower == "stop_"
        || lower == "global_"
        || lower.starts_with("data_")
        || lower.starts_with("save_")
        || lower.starts_with('_')
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if line.starts_with(';') {
            let mut block = String::new();
            for block_line in lines.by_ref() {
                if block_line.starts_with(';') {
                    break;
                }
                if !block.is_empty() {
                    block.push('\n');
                }
                block.push_str(block_line);
            }
            tokens.push(block);
            continue;
        }

        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i >= chars.len() || chars[i] == '#' {
                break;
            }
            if chars[i] == '\'' || chars[i] == '"' {
                let quote = chars[i];
                i += 1;
                let start = i;
                while i < chars.len() && chars[i] != quote {
                    i += 1;
                }
                tokens.push(chars[start..i].iter().collect());
                i = (i + 1).min(chars.len());
            } else {
                let start = i;
                while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '#' {
                    i += 1;
                }
                tokens.push(chars[start..i].iter().collect());
                if i < chars.len() && chars[i] == '#' {
                    break;
                }
            }
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_symmetry_expression() {
        let operation = parse_symmetry_operation("-x+1/2, y+1/2, z").unwrap();
        let actual = operation.apply(Vec3::new(0.1, 0.2, 0.3));
        assert!((actual.x - 0.4).abs() < 1.0e-5);
        assert!((actual.y - 0.7).abs() < 1.0e-5);
        assert!((actual.z - 0.3).abs() < 1.0e-5);
    }

    #[test]
    fn parses_minimal_cif_and_expands_symmetry() {
        let text = r#"
data_demo
_cell_length_a 5.0
_cell_length_b 5.0
_cell_length_c 5.0
_cell_angle_alpha 90
_cell_angle_beta 90
_cell_angle_gamma 90
loop_
_space_group_symop_operation_xyz
'x,y,z'
'-x,-y,-z'
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
Si1 Si 0.1 0.2 0.3
"#;
        let structure = parse(text).unwrap();
        assert_eq!(structure.atoms.len(), 2);
        assert_eq!(structure.atoms[0].element, "Si");
    }

    #[test]
    fn expands_from_space_group_metadata() {
        let text = r#"
data_inversion
_cell_length_a 5
_cell_length_b 5
_cell_length_c 5
_cell_angle_alpha 90
_cell_angle_beta 90
_cell_angle_gamma 90
_space_group_IT_number 2
_space_group_name_H-M_alt 'P -1'
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
C1 C 0.1 0.2 0.3
"#;
        let structure = parse(text).unwrap();
        assert_eq!(structure.asymmetric_atoms.len(), 1);
        assert_eq!(structure.atoms.len(), 2);
        assert_eq!(structure.symmetry_operation_count, 2);
    }

    #[test]
    fn infers_operations_from_space_group_symbol_without_it_number() {
        let text = r#"
data_symbol_only
_cell_length_a 5
_cell_length_b 5
_cell_length_c 5
_cell_angle_alpha 90
_cell_angle_beta 90
_cell_angle_gamma 90
_symmetry_space_group_name_H-M 'P -1'
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
C1 C 0.1 0.2 0.3
"#;
        let structure = parse(text).unwrap();
        assert_eq!(structure.atoms.len(), 2);
        assert_eq!(structure.symmetry_operation_count, 2);
    }

    #[test]
    fn parses_mcif_moments_and_time_reversal() {
        let text = r#"
data_antiferromagnet
_cell_length_a 5
_cell_length_b 5
_cell_length_c 5
_cell_angle_alpha 90
_cell_angle_beta 90
_cell_angle_gamma 90
loop_
_space_group_symop_magn_operation.id
_space_group_symop_magn_operation.xyz
1 x,y,z,+1
2 -x,-y,-z,-1
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
Fe1 Fe 0.1 0.2 0.3
loop_
_atom_site_moment.label
_atom_site_moment.crystalaxis_x
_atom_site_moment.crystalaxis_y
_atom_site_moment.crystalaxis_z
Fe1 0 0 3.5
"#;
        let structure = parse(text).unwrap();
        assert_eq!(structure.atoms.len(), 2);
        assert_eq!(structure.symmetry_operation_count, 2);
        assert!((structure.atoms[0].magnetic_moment.unwrap().z - 3.5).abs() < 1.0e-5);
        assert!((structure.atoms[1].magnetic_moment.unwrap().z + 3.5).abs() < 1.0e-5);
    }
}
