use std::collections::HashMap;

use bevy::prelude::{Mat3, Vec3};
use cryspglib::Crystal;
use cryspglib::spg_database::{SPACEGROUP_TYPES, get_spacegroup_operations};

use crate::{
    elements::normalize_symbol,
    model::{AtomSite, CrystalStructure, SiteId, SourceFormat, SymmetryMetadata, UnitCell},
};

use super::{ParseError, StructureCandidate, parse_number};

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

#[cfg(test)]
fn parse(text: &str) -> Result<CrystalStructure, ParseError> {
    parse_candidates(text).map(|mut candidates| candidates.remove(0).structure)
}

pub fn parse_candidates(text: &str) -> Result<Vec<StructureCandidate>, ParseError> {
    let tokens = tokenize(text);
    let mut candidates = Vec::new();
    for block in data_blocks(&tokens) {
        let (scalars, loops, data_name) = parse_tokens(block);
        if is_structure_block(&scalars, &loops)
            && let Ok(structure) = parse_block(block)
        {
            candidates.push(StructureCandidate {
                data_name,
                structure,
            });
        }
    }
    if candidates.is_empty() {
        return parse_block(&tokens).map(|structure| {
            vec![StructureCandidate {
                data_name: String::new(),
                structure,
            }]
        });
    }
    Ok(candidates)
}

fn parse_block(tokens: &[String]) -> Result<CrystalStructure, ParseError> {
    let (scalars, loops, data_name) = parse_tokens(tokens);
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
    let u_iso_index = index(&["_atom_site_u_iso_or_equiv"]);
    let b_iso_index = index(&["_atom_site_b_iso_or_equiv"]);
    let disorder_assembly_index = index(&["_atom_site_disorder_assembly"]);
    let disorder_group_index = index(&["_atom_site_disorder_group"]);

    let mut warnings = Vec::new();
    let magnetic_moments = magnetic_moments(&loops, &cell, &mut warnings);
    let anisotropic_displacements = anisotropic_displacements(&loops, &mut warnings);
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
        let u_iso = u_iso_index
            .and_then(|i| row.get(i))
            .and_then(|value| parse_number(value, "_atom_site_u_iso_or_equiv").ok())
            .or_else(|| {
                b_iso_index
                    .and_then(|i| row.get(i))
                    .and_then(|value| parse_number(value, "_atom_site_b_iso_or_equiv").ok())
                    .map(|b_iso| b_iso / (8.0 * std::f32::consts::PI.powi(2)))
            });
        let disorder_assembly = disorder_assembly_index
            .and_then(|i| row.get(i))
            .and_then(|value| clean_text(value));
        let disorder_group = disorder_group_index
            .and_then(|i| row.get(i))
            .and_then(|value| clean_text(value));
        let magnetic_moment = magnetic_moments.get(&label).copied();
        let anisotropic_u = anisotropic_displacements.get(&label).copied();
        base_atoms.push(AtomSite {
            site_id: SiteId(base_atoms.len() as u64),
            element: normalize_symbol(raw_symbol),
            label,
            fractional: wrap01(fractional),
            fractional_as_written: fractional,
            occupancy,
            u_iso,
            anisotropic_u,
            wyckoff_position: None,
            site_symmetry: None,
            disorder_assembly,
            disorder_group,
            symmetry_rotation: Mat3::IDENTITY,
            symmetry_time_reversal: 1.0,
            magnetic_moment,
        });
    }

    if base_atoms.is_empty() {
        return Err(ParseError::NoAtoms);
    }

    let space_group = space_group_label(&scalars);
    let symmetry_metadata = symmetry_metadata(&scalars, space_group.as_deref());
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
    annotate_wyckoff_sites(
        &cell,
        &mut base_atoms,
        &mut atoms,
        &operations,
        is_magnetic,
        &mut warnings,
    );

    let title = title_from_metadata(&scalars, &data_name)
        .unwrap_or_else(|| "Untitled structure".to_string());

    Ok(CrystalStructure {
        title,
        cell,
        asymmetric_atoms: base_atoms,
        atoms,
        space_group,
        symmetry_metadata,
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

fn symmetry_metadata(
    scalars: &HashMap<String, String>,
    space_group_name: Option<&str>,
) -> SymmetryMetadata {
    let magnetic_to_standard = scalar_text(scalars, "_space_group_magn.transform_bns_pp_abc");
    SymmetryMetadata {
        lattice_centering: space_group_name.and_then(lattice_centering),
        space_group_number: space_group_number(scalars).and_then(|value| u16::try_from(value).ok()),
        setting: scalar_text(scalars, "_space_group_it_coordinate_system_code")
            .or_else(|| scalar_text(scalars, "_space_group.it_coordinate_system_code"))
            // The conventional first setting is what VESTA reports when a CIF
            // does not specify a coordinate-system choice.
            .or_else(|| space_group_name.map(|_| "1".to_string())),
        parent_space_group_name: scalar_text(scalars, "_parent_space_group.name_h-m_alt"),
        parent_space_group_number: scalar_text(scalars, "_parent_space_group.it_number")
            .and_then(|value| value.parse::<u16>().ok()),
        magnetic_space_group_name_bns: scalar_text(scalars, "_space_group_magn.name_bns"),
        magnetic_space_group_number_bns: scalar_text(scalars, "_space_group_magn.number_bns"),
        transformation_from_standard: magnetic_to_standard
            .as_deref()
            .and_then(inverse_jones_faithful_transform),
        transformation_to_standard: magnetic_to_standard,
    }
}

fn scalar_text(scalars: &HashMap<String, String>, tag: &str) -> Option<String> {
    scalars.get(tag).and_then(|value| clean_text(value))
}

fn clean_text(value: &str) -> Option<String> {
    let value = value.trim_matches(['\'', '"']).trim();
    (!value.is_empty() && value != "." && value != "?").then(|| value.to_string())
}

fn title_from_metadata(scalars: &HashMap<String, String>, data_name: &str) -> Option<String> {
    [
        "_chemical_name_common",
        "_chemical_name_systematic",
        "_chemical_name_mineral",
        "_chemical_formula_sum",
        "_chemical_formula_moiety",
    ]
    .into_iter()
    .find_map(|tag| scalar_text(scalars, tag))
    .or_else(|| {
        let name = data_name.trim().replace('_', " ");
        (!name.is_empty() && name != "." && name != "?").then_some(name)
    })
}

fn lattice_centering(space_group_name: &str) -> Option<String> {
    space_group_name
        .chars()
        .find(|character| character.is_ascii_alphabetic())
        .map(|character| character.to_ascii_uppercase().to_string())
        .filter(|centering| matches!(centering.as_str(), "P" | "A" | "B" | "C" | "I" | "F" | "R"))
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
            let value = value.trim_matches(['\'', '"']).trim();
            if !value.is_empty() && value != "?" && value != "." {
                return Some(value.to_string());
            }
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

fn anisotropic_displacements(
    loops: &[LoopData],
    warnings: &mut Vec<String>,
) -> HashMap<String, [[f32; 3]; 3]> {
    let Some(loop_data) = loops.iter().find(|loop_data| {
        loop_data
            .tags
            .iter()
            .any(|tag| tag == "_atom_site_aniso_label")
    }) else {
        return HashMap::new();
    };
    let index = |tag: &str| loop_data.tags.iter().position(|candidate| candidate == tag);
    let Some(label_index) = index("_atom_site_aniso_label") else {
        return HashMap::new();
    };
    let u_tags = [
        "_atom_site_aniso_u_11",
        "_atom_site_aniso_u_22",
        "_atom_site_aniso_u_33",
        "_atom_site_aniso_u_12",
        "_atom_site_aniso_u_13",
        "_atom_site_aniso_u_23",
    ];
    let b_tags = [
        "_atom_site_aniso_b_11",
        "_atom_site_aniso_b_22",
        "_atom_site_aniso_b_33",
        "_atom_site_aniso_b_12",
        "_atom_site_aniso_b_13",
        "_atom_site_aniso_b_23",
    ];
    let (indices, b_factor) = if let Some(indices) = u_tags
        .iter()
        .map(|tag| index(tag))
        .collect::<Option<Vec<_>>>()
    {
        (indices, 1.0)
    } else if let Some(indices) = b_tags
        .iter()
        .map(|tag| index(tag))
        .collect::<Option<Vec<_>>>()
    {
        (indices, 1.0 / (8.0 * std::f32::consts::PI.powi(2)))
    } else {
        return HashMap::new();
    };
    let mut displacements = HashMap::new();
    for row in loop_data.values.chunks(loop_data.tags.len()) {
        let Some(label) = row.get(label_index).and_then(|value| clean_text(value)) else {
            continue;
        };
        let Some(values) = indices
            .iter()
            .map(|&index| {
                row.get(index)
                    .and_then(|value| parse_number(value, "anisotropic U").ok())
            })
            .collect::<Option<Vec<_>>>()
        else {
            warnings.push(format!(
                "Ignored incomplete anisotropic displacement for site '{label}'."
            ));
            continue;
        };
        let values: Vec<_> = values.into_iter().map(|value| value * b_factor).collect();
        displacements.insert(
            label,
            [
                [values[0], values[3], values[4]],
                [values[3], values[1], values[5]],
                [values[4], values[5], values[2]],
            ],
        );
    }
    displacements
}

fn annotate_wyckoff_sites(
    cell: &UnitCell,
    asymmetric_atoms: &mut [AtomSite],
    atoms: &mut [AtomSite],
    operations: &[SymmetryOperation],
    is_magnetic: bool,
    warnings: &mut Vec<String>,
) {
    if atoms.is_empty() || atoms.len() > 20_000 {
        return;
    }
    let lattice = [
        [
            cell.vectors[0].x as f64,
            cell.vectors[1].x as f64,
            cell.vectors[2].x as f64,
        ],
        [
            cell.vectors[0].y as f64,
            cell.vectors[1].y as f64,
            cell.vectors[2].y as f64,
        ],
        [
            cell.vectors[0].z as f64,
            cell.vectors[1].z as f64,
            cell.vectors[2].z as f64,
        ],
    ];
    let mut type_ids = HashMap::new();
    let types = atoms
        .iter()
        .map(|atom| {
            let next = type_ids.len() as i32 + 1;
            *type_ids.entry(atom.element.clone()).or_insert(next)
        })
        .collect();
    let positions = atoms
        .iter()
        .map(|atom| {
            [
                atom.fractional.x as f64,
                atom.fractional.y as f64,
                atom.fractional.z as f64,
            ]
        })
        .collect();
    let Ok(crystal) = Crystal::new(lattice, positions, types) else {
        return;
    };
    let Ok(dataset) = crystal.analyze().symprec(1.0e-4).dataset() else {
        warnings.push("Could not determine Wyckoff positions for this CIF setting.".to_string());
        return;
    };
    if dataset.wyckoffs.len() != atoms.len() || dataset.site_symmetry_symbols.len() != atoms.len() {
        return;
    }
    let mut multiplicities = HashMap::new();
    for orbit in &dataset.equivalent_atoms {
        *multiplicities.entry(*orbit).or_insert(0_usize) += 1;
    }
    let mut by_site = HashMap::new();
    for (index, atom) in atoms.iter_mut().enumerate() {
        let multiplicity = multiplicities[&dataset.equivalent_atoms[index]];
        let wyckoff = format!("{multiplicity}{}", dataset.wyckoffs[index].as_char());
        let symmetry = dataset.site_symmetry_symbols[index].trim().to_string();
        atom.wyckoff_position = Some(wyckoff.clone());
        atom.site_symmetry = (!symmetry.is_empty()).then_some(symmetry.clone());
        by_site.entry(atom.site_id).or_insert((wyckoff, symmetry));
    }
    for atom in asymmetric_atoms {
        if let Some((wyckoff, symmetry)) = by_site.get(&atom.site_id) {
            atom.wyckoff_position = Some(wyckoff.clone());
            atom.site_symmetry = if is_magnetic {
                magnetic_site_symmetry(atom.fractional, operations)
                    .or_else(|| (!symmetry.is_empty()).then(|| symmetry.clone()))
            } else {
                (!symmetry.is_empty()).then(|| symmetry.clone())
            };
        }
    }
}

/// Build the conventional dotted site-symmetry notation for orthorhombic-like
/// magnetic stabilizers. The prime marks operations accompanied by time
/// reversal. Other crystal systems retain the non-magnetic symbol determined
/// by spglib rather than risking an incorrect prime placement.
fn magnetic_site_symmetry(position: Vec3, operations: &[SymmetryOperation]) -> Option<String> {
    let mut axis_symbols: [(Option<bool>, Option<bool>); 3] = [(None, None); 3];
    let mut inversion = None;
    let mut has_non_diagonal = false;
    for operation in operations {
        if periodic_distance(operation.apply(position), wrap01(position)) > 1.0e-4 {
            continue;
        }
        let matrix = operation.matrix;
        let rows = [
            [matrix.x_axis.x, matrix.y_axis.x, matrix.z_axis.x],
            [matrix.x_axis.y, matrix.y_axis.y, matrix.z_axis.y],
            [matrix.x_axis.z, matrix.y_axis.z, matrix.z_axis.z],
        ];
        if rows.iter().enumerate().any(|(row, values)| {
            values
                .iter()
                .enumerate()
                .any(|(column, value)| row != column && value.abs() > 1.0e-4)
        }) {
            has_non_diagonal = true;
            continue;
        }
        let diagonal = [rows[0][0], rows[1][1], rows[2][2]];
        if diagonal.iter().all(|value| (*value + 1.0).abs() < 1.0e-4) {
            inversion = Some(operation.time_reversal < 0.0);
            continue;
        }
        let negatives = diagonal.iter().filter(|value| **value < 0.0).count();
        match negatives {
            1 => {
                let axis = diagonal
                    .iter()
                    .position(|value| *value < 0.0)
                    .expect("one negative diagonal exists");
                axis_symbols[axis].1 = Some(operation.time_reversal < 0.0);
            }
            2 => {
                let axis = diagonal
                    .iter()
                    .position(|value| *value > 0.0)
                    .expect("one positive diagonal exists");
                axis_symbols[axis].0 = Some(operation.time_reversal < 0.0);
            }
            _ => {}
        }
    }
    if has_non_diagonal {
        return None;
    }
    if let Some(time_reversed) = inversion {
        return Some(if time_reversed {
            "-1'".to_string()
        } else {
            "-1".to_string()
        });
    }
    if axis_symbols
        .iter()
        .all(|(two, mirror)| two.is_none() && mirror.is_none())
    {
        return Some("1".to_string());
    }
    let mut output = String::new();
    for (two, mirror) in axis_symbols {
        match (two, mirror) {
            (Some(two_prime), Some(mirror_prime)) => {
                output.push('2');
                if two_prime {
                    output.push('\'');
                }
                output.push('/');
                output.push('m');
                if mirror_prime {
                    output.push('\'');
                }
            }
            (Some(two_prime), None) => {
                output.push('2');
                if two_prime {
                    output.push('\'');
                }
            }
            (None, Some(mirror_prime)) => {
                output.push('m');
                if mirror_prime {
                    output.push('\'');
                }
            }
            (None, None) => output.push('.'),
        }
    }
    Some(output)
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

/// Invert a Jones-Faithful basis/origin transformation. magCIF writes the
/// transformed basis vectors as rows of `P` and its origin shift in the
/// current basis, so the reverse shift is `-P p` modulo one.
fn inverse_jones_faithful_transform(value: &str) -> Option<String> {
    let (basis, origin) = value.trim().trim_matches(['\'', '"']).split_once(';')?;
    let basis: Vec<_> = basis.split(',').map(str::trim).collect();
    let origin: Vec<_> = origin.split(',').map(str::trim).collect();
    if basis.len() != 3 || origin.len() != 3 {
        return None;
    }
    let rows = [
        parse_basis_expression(basis[0])?,
        parse_basis_expression(basis[1])?,
        parse_basis_expression(basis[2])?,
    ];
    let origin = Vec3::new(
        parse_fraction(origin[0]).ok()?,
        parse_fraction(origin[1]).ok()?,
        parse_fraction(origin[2]).ok()?,
    );
    let matrix = Mat3::from_cols_array(&[
        rows[0][0], rows[1][0], rows[2][0], rows[0][1], rows[1][1], rows[2][1], rows[0][2],
        rows[1][2], rows[2][2],
    ]);
    if matrix.determinant().abs() < 1.0e-6 {
        return None;
    }
    let inverse = matrix.inverse();
    let inverse_rows = [
        [inverse.x_axis.x, inverse.y_axis.x, inverse.z_axis.x],
        [inverse.x_axis.y, inverse.y_axis.y, inverse.z_axis.y],
        [inverse.x_axis.z, inverse.y_axis.z, inverse.z_axis.z],
    ];
    let reverse_origin = -(matrix * origin);
    Some(format!(
        "{},{},{};{},{},{}",
        format_basis_expression(inverse_rows[0]),
        format_basis_expression(inverse_rows[1]),
        format_basis_expression(inverse_rows[2]),
        format_unit_fraction(reverse_origin.x),
        format_unit_fraction(reverse_origin.y),
        format_unit_fraction(reverse_origin.z),
    ))
}

fn parse_basis_expression(expression: &str) -> Option<[f32; 3]> {
    let mut normalized = expression.replace(' ', "").replace('-', "+-");
    if normalized.starts_with("+-") {
        normalized.remove(0);
    }
    let mut coefficients = [0.0; 3];
    for term in normalized.split('+').filter(|term| !term.is_empty()) {
        let (index, axis) = term
            .char_indices()
            .find(|(_, character)| matches!(character, 'a' | 'b' | 'c'))?;
        if !term[index + axis.len_utf8()..].is_empty() {
            return None;
        }
        let coefficient = match &term[..index] {
            "" => 1.0,
            "-" => -1.0,
            value => parse_fraction(value).ok()?,
        };
        coefficients[match axis {
            'a' => 0,
            'b' => 1,
            'c' => 2,
            _ => unreachable!(),
        }] += coefficient;
    }
    Some(coefficients)
}

fn format_basis_expression(coefficients: [f32; 3]) -> String {
    let mut output = String::new();
    for (axis, coefficient) in ['a', 'b', 'c'].into_iter().zip(coefficients) {
        if coefficient.abs() < 1.0e-5 {
            continue;
        }
        let negative = coefficient < 0.0;
        if !output.is_empty() {
            output.push(if negative { '-' } else { '+' });
        } else if negative {
            output.push('-');
        }
        let magnitude = coefficient.abs();
        if (magnitude - 1.0).abs() > 1.0e-5 {
            output.push_str(&format_fraction(magnitude));
        }
        output.push(axis);
    }
    if output.is_empty() {
        "0".to_string()
    } else {
        output
    }
}

fn format_unit_fraction(value: f32) -> String {
    format_fraction(value.rem_euclid(1.0))
}

fn format_fraction(value: f32) -> String {
    const DENOMINATORS: [i32; 8] = [1, 2, 3, 4, 6, 8, 12, 24];
    for denominator in DENOMINATORS {
        let numerator = (value * denominator as f32).round() as i32;
        if ((numerator as f32 / denominator as f32) - value).abs() < 1.0e-4 {
            return if denominator == 1 {
                numerator.to_string()
            } else {
                format!("{numerator}/{denominator}")
            };
        }
    }
    format!("{value:.5}")
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

/// CIF files may carry multiple `data_` blocks, for example standardized,
/// published, and reduced cells. Select a self-contained structure block
/// rather than merging unrelated blocks and allowing later placeholder values
/// to overwrite its space-group metadata.
fn data_blocks(tokens: &[String]) -> Vec<&[String]> {
    let starts: Vec<_> = tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| {
            token
                .to_ascii_lowercase()
                .starts_with("data_")
                .then_some(index)
        })
        .collect();
    if starts.is_empty() {
        return vec![tokens];
    }
    starts
        .iter()
        .enumerate()
        .map(|(index, &start)| {
            let end = starts.get(index + 1).copied().unwrap_or(tokens.len());
            &tokens[start..end]
        })
        .collect()
}

fn is_structure_block(scalars: &HashMap<String, String>, loops: &[LoopData]) -> bool {
    ["_cell_length_a", "_cell_length_b", "_cell_length_c"]
        .into_iter()
        .all(|tag| scalars.contains_key(tag))
        && loops.iter().any(|loop_data| {
            [
                "_atom_site_fract_x",
                "_atom_site_fract_y",
                "_atom_site_fract_z",
            ]
            .into_iter()
            .all(|tag| loop_data.tags.iter().any(|candidate| candidate == tag))
        })
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
    fn selects_the_first_non_placeholder_title_candidate() {
        let text = r#"
data_fallback_block
_cell_length_a 5
_cell_length_b 5
_cell_length_c 5
_chemical_name_common ?
_chemical_name_systematic .
_chemical_name_mineral ?
_chemical_formula_sum 'Na2 Te O6'
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
Na1 Na 0 0 0
"#;
        assert_eq!(parse(text).unwrap().title, "Na2 Te O6");
    }

    #[test]
    fn preserves_mixed_occupancy_and_disorder_metadata() {
        let text = r#"
data_mixed_site
_cell_length_a 5
_cell_length_b 5
_cell_length_c 5
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
_atom_site_occupancy
_atom_site_disorder_assembly
_atom_site_disorder_group
Na1 Na 0 0 0 0.67 A 1
K1 K 0 0 0 0.33 A 1
"#;
        let structure = parse(text).unwrap();
        assert_eq!(structure.asymmetric_atoms.len(), 2);
        assert!((structure.asymmetric_atoms[0].occupancy - 0.67).abs() < 1.0e-5);
        assert_eq!(
            structure.asymmetric_atoms[1].disorder_assembly.as_deref(),
            Some("A")
        );
        assert_eq!(
            structure.asymmetric_atoms[1].disorder_group.as_deref(),
            Some("1")
        );
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
    fn selects_the_complete_structure_block_from_multi_block_cif() {
        let text = r#"
data_metadata
_symmetry_Int_Tables_number 62

data_standardized_cell
_cell_length_a 13.937
_cell_length_b 8.1357
_cell_length_c 6.5636
_symmetry_space_group_name_H-M 'Pnma'
_symmetry_Int_Tables_number 62
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
Mn1 Mn 0 0 0

data_niggli_cell
_cell_length_a 6.5636
_cell_length_b 8.1357
_cell_length_c 13.937
_symmetry_space_group_name_H-M '?'
_symmetry_Int_Tables_number ?
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
? ? ? ? ?
"#;
        let structure = parse(text).expect("standardized block is valid");
        assert_eq!(structure.space_group.as_deref(), Some("Pnma"));
        assert_eq!(structure.symmetry_operation_count, 8);
        assert_eq!(structure.asymmetric_atoms[0].element, "Mn");
    }

    #[test]
    fn returns_each_valid_structure_block_as_a_candidate() {
        let text = r#"
data_first
_cell_length_a 5
_cell_length_b 5
_cell_length_c 5
_symmetry_space_group_name_H-M 'P 1'
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
C1 C 0 0 0

data_second
_cell_length_a 6
_cell_length_b 6
_cell_length_c 6
_symmetry_space_group_name_H-M 'P -1'
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
Si1 Si 0.1 0.2 0.3
"#;
        let candidates = parse_candidates(text).expect("both blocks are valid");
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].data_name, "first");
        assert_eq!(candidates[1].data_name, "second");
        assert_eq!(candidates[1].structure.space_group.as_deref(), Some("P -1"));
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

    #[test]
    fn preserves_magnetic_space_group_report_metadata() {
        let text = r#"
data_magnetic_setting
_cell_length_a 8
_cell_length_b 17
_cell_length_c 5
_parent_space_group.name_H-M_alt 'C m c m'
_parent_space_group.IT_number 63
_space_group_magn.transform_BNS_Pp_abc 'b,c,a;1/4,1/4,0'
_space_group_magn.number_BNS 52.318
_space_group_magn.name_BNS 'P_B n n a'
loop_
_space_group_symop_magn_operation.id
_space_group_symop_magn_operation.xyz
1 x,y,z,+1
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
Y1 Y 0 0 0
"#;
        let structure = parse(text).unwrap();
        let metadata = &structure.symmetry_metadata;
        assert_eq!(metadata.parent_space_group_name.as_deref(), Some("C m c m"));
        assert_eq!(metadata.parent_space_group_number, Some(63));
        assert_eq!(
            metadata.magnetic_space_group_name_bns.as_deref(),
            Some("P_B n n a")
        );
        assert_eq!(
            metadata.magnetic_space_group_number_bns.as_deref(),
            Some("52.318")
        );
        assert_eq!(
            metadata.transformation_to_standard.as_deref(),
            Some("b,c,a;1/4,1/4,0")
        );
        assert_eq!(
            metadata.transformation_from_standard.as_deref(),
            Some("c,a,b;3/4,0,3/4")
        );
    }

    #[test]
    fn formats_primed_magnetic_site_symmetry() {
        let operation = |diagonal: Vec3, time_reversal| SymmetryOperation {
            matrix: Mat3::from_diagonal(diagonal),
            translation: Vec3::ZERO,
            time_reversal,
        };
        let operations = [
            operation(Vec3::ONE, 1.0),
            operation(Vec3::new(-1.0, -1.0, 1.0), 1.0),
            operation(Vec3::new(1.0, 1.0, -1.0), -1.0),
        ];
        assert_eq!(
            magnetic_site_symmetry(Vec3::ZERO, &operations).as_deref(),
            Some("..2/m'")
        );
    }
}
