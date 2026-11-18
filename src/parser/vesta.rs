use bevy::prelude::{Mat3, Vec3};

use crate::{
    elements::normalize_symbol,
    model::{
        AtomSite, CrystalStructure, ImportedBondRule, ImportedVector, ImportedVisuals, SiteId,
        SourceFormat, SymmetryMetadata, UnitCell,
    },
};

use super::{ParseError, parse_number};

const SECTION_NAMES: &[&str] = &[
    "TITLE",
    "GROUP",
    "SYMOP",
    "TRANM",
    "LTRANSL",
    "LORIENT",
    "LMATRIX",
    "CELLP",
    "STRUC",
    "THERI",
    "SITET",
    "VECTR",
    "VECTT",
    "SBOND",
    "HBOND",
    "BOUND",
    "STYLE",
    "SCENE",
    "ATOMT",
    "ATOMM",
    "BONDM",
    "POLYM",
    "BKGRC",
    "FORMM",
    "ISURF",
    "IMPORT_STRUCTURE",
    "IMPORT_DENSITY",
    "CRYSTAL",
];

pub fn parse(text: &str) -> Result<CrystalStructure, ParseError> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let title = value_after_section(&lines, "TITLE")
        .filter(|value| !is_section(value))
        .unwrap_or("Untitled VESTA structure")
        .to_string();

    let cell_index = find_section(&lines, "CELLP").ok_or(ParseError::MissingField("CELLP"))?;
    let cell_values = next_numeric_line(&lines, cell_index + 1)
        .ok_or(ParseError::MissingField("CELLP values"))?;
    if cell_values.len() < 6 {
        return Err(ParseError::InvalidStructure(
            "CELLP must contain six lattice parameters".to_string(),
        ));
    }
    let cell = UnitCell::new(
        parse_number(cell_values[0], "CELLP a")?,
        parse_number(cell_values[1], "CELLP b")?,
        parse_number(cell_values[2], "CELLP c")?,
        parse_number(cell_values[3], "CELLP alpha")?,
        parse_number(cell_values[4], "CELLP beta")?,
        parse_number(cell_values[5], "CELLP gamma")?,
    );

    let structure_index = find_section(&lines, "STRUC").ok_or(ParseError::MissingField("STRUC"))?;
    let mut atoms = Vec::new();
    let mut index = structure_index + 1;
    while index < lines.len() {
        let line = lines[index];
        if line.is_empty() {
            index += 1;
            continue;
        }
        if is_section(line) {
            break;
        }
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.first() == Some(&"0") || parts.first() == Some(&"-1") {
            break;
        }
        if parts.len() >= 7 && parts[0].parse::<usize>().is_ok() {
            let atom = AtomSite {
                site_id: SiteId(atoms.len() as u64),
                element: normalize_symbol(parts[1]),
                label: parts[2].to_string(),
                occupancy: parse_number(parts[3], "STRUC occupancy").unwrap_or(1.0),
                disorder_assembly: None,
                disorder_group: None,
                fractional: Vec3::new(
                    parse_number(parts[4], "STRUC x")?,
                    parse_number(parts[5], "STRUC y")?,
                    parse_number(parts[6], "STRUC z")?,
                ),
                fractional_as_written: Vec3::new(
                    parse_number(parts[4], "STRUC x")?,
                    parse_number(parts[5], "STRUC y")?,
                    parse_number(parts[6], "STRUC z")?,
                ),
                symmetry_rotation: Mat3::IDENTITY,
                symmetry_time_reversal: 1.0,
                u_iso: None,
                anisotropic_u: None,
                wyckoff_position: None,
                site_symmetry: None,
                magnetic_moment: None,
            };
            atoms.push(atom);
            // VESTA stores displacement parameters on the line following each atom.
            index += 2;
        } else {
            index += 1;
        }
    }

    if atoms.is_empty() {
        return Err(ParseError::NoAtoms);
    }
    let asymmetric_atoms = atoms;
    let symmetry_operations = vesta_symmetry_operations(&lines);
    let atoms = expand_vesta_symmetry(&asymmetric_atoms, &symmetry_operations);
    let imported_visuals = parse_imported_visuals(&lines, &asymmetric_atoms);
    let mut warnings = Vec::new();
    if symmetry_operations.len() > 1 {
        warnings.push(format!(
            "Expanded {} VESTA symmetry operation(s).",
            symmetry_operations.len()
        ));
    }
    if !imported_visuals.vectors.is_empty() {
        warnings.push(format!(
            "Loaded {} VESTA vector(s) from VECTR/VECTT.",
            imported_visuals.vectors.len()
        ));
    }
    if !imported_visuals.bond_rules.is_empty() {
        warnings.push(format!(
            "Loaded {} VESTA bond rule(s) from SBOND.",
            imported_visuals.bond_rules.len()
        ));
    }
    warnings.push(
        "VESTA density, isosurface, and unsupported style settings are not imported yet."
            .to_string(),
    );

    Ok(CrystalStructure {
        title,
        cell,
        asymmetric_atoms,
        atoms,
        space_group: value_after_section(&lines, "GROUP").map(str::to_string),
        symmetry_metadata: SymmetryMetadata::default(),
        symmetry_operation_count: symmetry_operations.len(),
        source_format: SourceFormat::Vesta,
        warnings,
        imported_visuals,
    })
}

fn find_section(lines: &[&str], name: &str) -> Option<usize> {
    lines
        .iter()
        .position(|line| line.eq_ignore_ascii_case(name))
}

fn value_after_section<'a>(lines: &'a [&str], name: &str) -> Option<&'a str> {
    let mut index = find_section(lines, name)? + 1;
    while index < lines.len() {
        if !lines[index].is_empty() {
            return Some(lines[index]);
        }
        index += 1;
    }
    None
}

fn section_lines<'a>(lines: &'a [&'a str], name: &str) -> Vec<&'a str> {
    let Some(start) = find_section(lines, name) else {
        return Vec::new();
    };
    lines[start + 1..]
        .iter()
        .take_while(|line| !is_section(line))
        .copied()
        .filter(|line| !line.is_empty())
        .collect()
}

#[derive(Clone, Copy)]
struct VestaSymmetryOperation {
    matrix: Mat3,
    translation: Vec3,
}

fn vesta_symmetry_operations(lines: &[&str]) -> Vec<VestaSymmetryOperation> {
    let mut operations = Vec::new();
    for line in section_lines(lines, "SYMOP") {
        let values: Vec<_> = line
            .split_whitespace()
            .filter_map(|value| value.parse::<f32>().ok())
            .collect();
        if values.len() < 12 || values[0..3].iter().all(|value| *value <= -0.999) {
            continue;
        }
        let rows = &values[3..12];
        operations.push(VestaSymmetryOperation {
            translation: Vec3::new(values[0], values[1], values[2]),
            matrix: Mat3::from_cols_array(&[
                rows[0], rows[3], rows[6], rows[1], rows[4], rows[7], rows[2], rows[5], rows[8],
            ]),
        });
    }
    if operations.is_empty() {
        operations.push(VestaSymmetryOperation {
            matrix: Mat3::IDENTITY,
            translation: Vec3::ZERO,
        });
    }
    operations
}

fn expand_vesta_symmetry(
    asymmetric_atoms: &[AtomSite],
    operations: &[VestaSymmetryOperation],
) -> Vec<AtomSite> {
    let mut atoms = Vec::new();
    for atom in asymmetric_atoms {
        for operation in operations {
            let fractional = wrap01(operation.matrix * atom.fractional + operation.translation);
            if atoms.iter().any(|existing: &AtomSite| {
                existing.element == atom.element
                    && periodic_distance(existing.fractional, fractional) < 1.0e-4
            }) {
                continue;
            }
            let mut expanded = atom.clone();
            expanded.fractional = fractional;
            expanded.symmetry_rotation = operation.matrix;
            atoms.push(expanded);
        }
    }
    atoms
}

fn parse_imported_visuals(lines: &[&str], atoms: &[AtomSite]) -> ImportedVisuals {
    let vector_scale = style_vector_scale(lines);
    let vector_styles = parse_vector_styles(lines);
    ImportedVisuals {
        background: parse_background(lines),
        bond_rules: parse_bond_rules(lines),
        vectors: parse_vectors(lines, atoms, vector_scale, &vector_styles),
    }
}

fn parse_background(lines: &[&str]) -> Option<[u8; 3]> {
    let values: Vec<_> = section_lines(lines, "BKGRC")
        .first()?
        .split_whitespace()
        .filter_map(|value| value.parse::<u8>().ok())
        .collect();
    (values.len() >= 3).then(|| [values[0], values[1], values[2]])
}

fn style_vector_scale(lines: &[&str]) -> f32 {
    section_lines(lines, "STYLE")
        .iter()
        .find_map(|line| {
            let mut values = line.split_whitespace();
            if values
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case("VECTS"))
            {
                values.next()?.parse::<f32>().ok()
            } else {
                None
            }
        })
        .unwrap_or(1.0)
}

#[derive(Clone, Copy)]
struct VectorStyle {
    color: [u8; 3],
    penetrate_atom: bool,
    add_atom_radius: bool,
}

fn parse_vector_styles(lines: &[&str]) -> std::collections::HashMap<usize, VectorStyle> {
    let mut styles = std::collections::HashMap::new();
    for line in section_lines(lines, "VECTT") {
        let values: Vec<_> = line.split_whitespace().collect();
        let Some(index) = values.first().and_then(|value| value.parse::<usize>().ok()) else {
            continue;
        };
        if index == 0 || values.len() < 6 {
            continue;
        }
        let color = match (
            values[2].parse::<u8>(),
            values[3].parse::<u8>(),
            values[4].parse::<u8>(),
        ) {
            (Ok(red), Ok(green), Ok(blue)) => [red, green, blue],
            _ => continue,
        };
        let kind = values[5].parse::<u8>().unwrap_or(2);
        styles.insert(
            index,
            VectorStyle {
                color,
                penetrate_atom: kind == 1 || kind == 3,
                add_atom_radius: kind == 2 || kind == 3,
            },
        );
    }
    styles
}

fn parse_vectors(
    lines: &[&str],
    atoms: &[AtomSite],
    scale: f32,
    styles: &std::collections::HashMap<usize, VectorStyle>,
) -> Vec<ImportedVector> {
    let entries = section_lines(lines, "VECTR");
    let mut vectors = Vec::new();
    let mut line_index = 0;
    while line_index + 1 < entries.len() {
        let values: Vec<_> = entries[line_index].split_whitespace().collect();
        let vector_index = values.first().and_then(|value| value.parse::<usize>().ok());
        if vector_index.is_none() || vector_index == Some(0) || values.len() < 4 {
            line_index += 1;
            continue;
        }
        let target: Vec<_> = entries[line_index + 1].split_whitespace().collect();
        let target_index = target.first().and_then(|value| value.parse::<usize>().ok());
        let components = (|| {
            Some(Vec3::new(
                values[1].parse().ok()?,
                values[2].parse().ok()?,
                values[3].parse().ok()?,
            ))
        })();
        if let (Some(vector_index), Some(target_index), Some(components)) =
            (vector_index, target_index, components)
            && let Some(atom) = atoms.get(target_index.saturating_sub(1))
        {
            let style = styles.get(&vector_index).copied().unwrap_or(VectorStyle {
                color: [237, 68, 83],
                penetrate_atom: true,
                add_atom_radius: true,
            });
            vectors.push(ImportedVector {
                label: atom.label.clone(),
                components,
                color: style.color,
                scale,
                penetrate_atom: style.penetrate_atom,
                add_atom_radius: style.add_atom_radius,
            });
        }
        // A VECTR entry has its endpoint row, target-atom row, and option row.
        line_index += 3;
    }
    vectors
}

fn parse_bond_rules(lines: &[&str]) -> Vec<ImportedBondRule> {
    section_lines(lines, "SBOND")
        .into_iter()
        .filter_map(|line| {
            let values: Vec<_> = line.split_whitespace().collect();
            let identifier = values.first()?.parse::<usize>().ok()?;
            if identifier == 0 || values.len() < 5 {
                return None;
            }
            Some(ImportedBondRule {
                center: values[1].to_string(),
                neighbor: values[2].to_string(),
                min_length: values[3].parse().ok()?,
                max_length: values[4].parse().ok()?,
                // In VESTA's SBOND records this flag follows the drawing-boundary fields.
                show_polyhedra: values.get(8).is_some_and(|value| *value == "1"),
            })
        })
        .collect()
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

fn next_numeric_line<'a>(lines: &'a [&'a str], mut index: usize) -> Option<Vec<&'a str>> {
    while index < lines.len() {
        let parts: Vec<_> = lines[index].split_whitespace().collect();
        if parts
            .first()
            .is_some_and(|value| value.parse::<f32>().is_ok())
        {
            return Some(parts);
        }
        if is_section(lines[index]) {
            return None;
        }
        index += 1;
    }
    None
}

fn is_section(value: &str) -> bool {
    let first = value.split_whitespace().next().unwrap_or_default();
    SECTION_NAMES
        .iter()
        .any(|section| first.eq_ignore_ascii_case(section))
        || first.starts_with("#VESTA_FORMAT_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vesta_geometry() {
        let text = r#"
#VESTA_FORMAT_VERSION 3.5.0
CRYSTAL
TITLE
Demo
CELLP
5.0 5.0 6.0 90.0 90.0 120.0
0 0 0 0 0 0
STRUC
1 Si Si1 1.0 0.1 0.2 0.3 1a 1
0.0 0.0 0.0 0.0
2 O O1 1.0 0.4 0.5 0.6 1a 1
0.0 0.0 0.0 0.0
0 0 0 0 0 0 0
STYLE
"#;
        let structure = parse(text).unwrap();
        assert_eq!(structure.title, "Demo");
        assert_eq!(structure.atoms.len(), 2);
        assert_eq!(structure.atoms[1].element, "O");
    }

    #[test]
    fn imports_vesta_visuals_and_symmetry() {
        let text = r#"
#VESTA_FORMAT_VERSION 3.5.0
CRYSTAL
TITLE
Demo
CELLP
5.0 5.0 5.0 90.0 90.0 90.0
0 0 0 0 0 0
STRUC
1 Fe Fe1 1.0 0.1 0.2 0.3 1a 1
0.0 0.0 0.0 0.0
2 O O1 1.0 0.4 0.5 0.6 1a 1
0.0 0.0 0.0 0.0
0 0 0 0 0 0 0
SYMOP
0 0 0 1 0 0 0 1 0 0 0 1 1
0 0 0 -1 0 0 0 -1 0 0 0 -1 1
-1 -1 -1 0 0 0 0 0 0 0 0 0
VECTR
1 0.0 0.0 2.0 0
1 0 0 0 0
0 0 0 0 0
VECTT
1 0.5 255 0 0 2
0 0 0 0 0
SBOND
1 Fe O 0.0 2.4 0 0 1 0 1
0 0 0 0
BKGRC
12 34 56
STYLE
VECTS 0.4
"#;
        let structure = parse(text).unwrap();
        assert_eq!(structure.atoms.len(), 4);
        assert_eq!(structure.symmetry_operation_count, 2);
        assert_eq!(structure.imported_visuals.background, Some([12, 34, 56]));
        assert_eq!(structure.imported_visuals.bond_rules.len(), 1);
        assert_eq!(structure.imported_visuals.vectors.len(), 1);
        let vector = &structure.imported_visuals.vectors[0];
        assert_eq!(vector.label, "Fe1");
        assert_eq!(vector.color, [255, 0, 0]);
        assert!((vector.components.z - 2.0).abs() < 1.0e-5);
        assert!((vector.scale - 0.4).abs() < 1.0e-5);
    }
}
