use std::{collections::HashMap, sync::OnceLock};

use bevy::prelude::Mat3;
use cryspglib::pointgroup_from_rotations;
use wyckoscope::model::{CrystalStructure, MagneticOperation};

const COMPONENTS: [(usize, usize, usize); 18] = [
    (0, 0, 0),
    (0, 0, 1),
    (0, 0, 2),
    (0, 1, 1),
    (0, 1, 2),
    (0, 2, 2),
    (1, 0, 0),
    (1, 0, 1),
    (1, 0, 2),
    (1, 1, 1),
    (1, 1, 2),
    (1, 2, 2),
    (2, 0, 0),
    (2, 0, 1),
    (2, 0, 2),
    (2, 1, 1),
    (2, 1, 2),
    (2, 2, 2),
];
const AXES: [&str; 3] = ["x", "y", "z"];
/// Bevy's `Mat3` is f32. Converting non-orthogonal crystal axes (notably
/// hexagonal axes) produces residuals of roughly 1e-7, so treating them as
/// exact constraints spuriously removes tensor components.
const EPSILON: f64 = 1.0e-5;
/// BNS magnetic-space-group number → UNI magnetic-point-group symbol.
///
/// This is a vendored 1651-entry table from `nolhallsym`; see
/// `assets/magnetic/NOTICE` for source and licence information.
const BNS_TO_MPG_TABLE: &str = include_str!("../assets/magnetic/msg_to_mpg.tsv");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TransportMechanism {
    Drude,
    Bcd,
    InterQmd,
    IntraQmd,
}

impl TransportMechanism {
    pub(crate) const ALL: [Self; 4] = [Self::Drude, Self::Bcd, Self::InterQmd, Self::IntraQmd];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Drude => "Drude (NLD)",
            Self::Bcd => "Berry-curvature dipole (BCD)",
            Self::InterQmd => "Interband QMD",
            Self::IntraQmd => "Intraband QMD",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MechanismAnalysis {
    pub(crate) mechanism: TransportMechanism,
    /// Each component expressed in a minimal set of symmetry-allowed
    /// parameters, e.g. `α`, `−α`, or `−2α`.
    pub(crate) expressions: [String; 18],
}

#[derive(Debug, Clone)]
pub(crate) struct LinearConductivityAnalysis {
    /// Row-major `σᵢⱼ` expressions.
    pub(crate) expressions: [String; 9],
}

#[derive(Debug, Clone)]
pub(crate) struct TransportAnalysis {
    pub(crate) magnetic_point_group: String,
    pub(crate) operation_count: usize,
    pub(crate) linear: LinearConductivityAnalysis,
    pub(crate) mechanisms: [MechanismAnalysis; 4],
}

pub(crate) fn analyze(structure: &CrystalStructure) -> TransportAnalysis {
    let operations = unique_operations(&structure.magnetic_operations);
    let cartesian: Vec<_> = operations
        .iter()
        .map(|operation| CartesianOperation {
            rotation: structure.cell.symmetry_to_cartesian(operation.rotation),
            time_reversal: operation.time_reversal,
        })
        .collect();
    let point_group = structure
        .symmetry_metadata
        .magnetic_space_group_number_bns
        .as_deref()
        .and_then(magnetic_point_group_from_bns)
        .unwrap_or_else(|| magnetic_group_label(&operations));
    TransportAnalysis {
        magnetic_point_group: point_group,
        operation_count: cartesian.len(),
        linear: analyze_linear_conductivity(&cartesian),
        mechanisms: TransportMechanism::ALL
            .map(|mechanism| analyze_mechanism(&cartesian, mechanism)),
    }
}

pub(crate) fn component_label(index: usize) -> String {
    let (i, j, k) = COMPONENTS[index];
    format!("σ_{};{}{}", AXES[i], AXES[j], AXES[k])
}

#[derive(Clone, Copy)]
struct CartesianOperation {
    rotation: Mat3,
    time_reversal: bool,
}

fn unique_operations(operations: &[MagneticOperation]) -> Vec<MagneticOperation> {
    let mut unique = Vec::new();
    for operation in operations {
        let duplicate = unique.iter().any(|known: &MagneticOperation| {
            known.time_reversal == operation.time_reversal
                && known
                    .rotation
                    .to_cols_array()
                    .iter()
                    .zip(operation.rotation.to_cols_array())
                    .all(|(left, right)| (*left - right).abs() < 1.0e-5)
        });
        if !duplicate {
            unique.push(operation.clone());
        }
    }
    if unique.is_empty() {
        unique.push(MagneticOperation {
            rotation: Mat3::IDENTITY,
            time_reversal: false,
        });
    }
    unique
}

/// Type-I and grey magnetic point groups have an unambiguous compact
/// Hermann--Mauguin spelling. For type-III groups, the placement of primes
/// depends on a convention-dependent axis choice, so preserve the exact
/// primed-operation count rather than reporting a misleading symbol.
fn magnetic_group_label(operations: &[MagneticOperation]) -> String {
    let spatial = spatial_point_group_symbol(operations)
        .unwrap_or_else(|| format!("unrecognised ({} spatial operation(s))", operations.len()));
    let primed_count = operations
        .iter()
        .filter(|operation| operation.time_reversal)
        .count();
    let has_pure_time_reversal = operations
        .iter()
        .any(|operation| operation.time_reversal && is_identity(operation.rotation));
    if has_pure_time_reversal {
        format!("{spatial}.1′")
    } else if primed_count == 0 {
        spatial
    } else {
        format!("{spatial} ({primed_count} primed point operation(s))")
    }
}

/// Looks up the canonical magnetic-point-group notation for a BNS MSG number.
/// The table uses ASCII apostrophes; the UI uses the standard prime glyph.
fn magnetic_point_group_from_bns(number: &str) -> Option<String> {
    let key = normalize_bns_number(number)?;
    bns_to_mpg_table()
        .get(key.as_str())
        .map(|symbol| symbol.replace('\'', "′"))
}

fn normalize_bns_number(number: &str) -> Option<String> {
    let mut parts = number.trim().split('.');
    let parent = parts.next()?.trim().parse::<u16>().ok()?;
    let sequence = parts.next()?.trim().parse::<u16>().ok()?;
    parts
        .next()
        .is_none()
        .then(|| format!("{parent}.{sequence}"))
}

fn bns_to_mpg_table() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| {
        BNS_TO_MPG_TABLE
            .lines()
            .skip(1)
            .filter_map(|line| {
                let columns: Vec<_> = line.split_whitespace().collect();
                (columns.len() >= 6).then(|| (columns[0], columns[5]))
            })
            .collect()
    })
}

fn spatial_point_group_symbol(operations: &[MagneticOperation]) -> Option<String> {
    let mut rotations = Vec::new();
    for operation in operations {
        let rotation = integer_rotation(operation.rotation)?;
        if !rotations.contains(&rotation) {
            rotations.push(rotation);
        }
    }
    pointgroup_from_rotations(&rotations)
        .ok()
        .map(|(symbol, _, _)| symbol)
}

fn integer_rotation(rotation: Mat3) -> Option<[[i32; 3]; 3]> {
    let values = rotation.to_cols_array();
    let mut integer = [[0; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            let value = values[column * 3 + row];
            let rounded = value.round();
            if (value - rounded).abs() > 1.0e-5 {
                return None;
            }
            integer[row][column] = rounded as i32;
        }
    }
    Some(integer)
}

fn is_identity(rotation: Mat3) -> bool {
    rotation
        .to_cols_array()
        .iter()
        .zip(Mat3::IDENTITY.to_cols_array())
        .all(|(left, right)| (*left - right).abs() < 1.0e-5)
}

fn analyze_mechanism(
    operations: &[CartesianOperation],
    mechanism: TransportMechanism,
) -> MechanismAnalysis {
    let basis = mechanism_basis(operations, mechanism);
    MechanismAnalysis {
        mechanism,
        expressions: std::array::from_fn(|component| component_expression(&basis, component)),
    }
}

fn mechanism_basis(
    operations: &[CartesianOperation],
    mechanism: TransportMechanism,
) -> Vec<[f64; 18]> {
    let mut equations = Vec::<[f64; 18]>::new();
    for operation in operations {
        let transform = tensor_transform(operation);
        // Equation (S132): only BCD gets η = -1 under an antiunitary symmetry.
        let eta = if mechanism == TransportMechanism::Bcd && operation.time_reversal {
            -1.0
        } else {
            1.0
        };
        for row_index in 0..18 {
            let mut row = [0.0; 18];
            row[row_index] = 1.0;
            for (column_index, coefficient) in row.iter_mut().enumerate() {
                *coefficient -= eta * transform[row_index][column_index];
            }
            equations.push(row);
        }
    }
    append_intrinsic_constraints(&mut equations, mechanism);
    nullspace(equations)
}

/// Applies Neumann's principle together with the Onsager relation. For a
/// unitary operation the conductivity obeys σ = RσRᵀ; for an antiunitary
/// operation it obeys σ = RσᵀRᵀ. The latter keeps longitudinal conductivity
/// time-even while making the Hall part time-odd.
fn analyze_linear_conductivity(operations: &[CartesianOperation]) -> LinearConductivityAnalysis {
    let basis = linear_conductivity_basis(operations);
    LinearConductivityAnalysis {
        expressions: std::array::from_fn(|component| component_expression(&basis, component)),
    }
}

fn linear_conductivity_basis(operations: &[CartesianOperation]) -> Vec<[f64; 9]> {
    let mut equations = Vec::<[f64; 9]>::new();
    for operation in operations {
        let rotation = spatial_rotation(operation.rotation);
        for output in 0..9 {
            let i = output / 3;
            let j = output % 3;
            let mut row = [0.0; 9];
            row[output] = 1.0;
            for (input, coefficient) in row.iter_mut().enumerate() {
                let k = input / 3;
                let l = input % 3;
                let (source_k, source_l) = if operation.time_reversal {
                    (l, k)
                } else {
                    (k, l)
                };
                *coefficient -= rotation[i][source_k] * rotation[j][source_l];
            }
            equations.push(row);
        }
    }
    nullspace(equations)
}

fn spatial_rotation(rotation: Mat3) -> [[f64; 3]; 3] {
    let raw = rotation.to_cols_array();
    let mut result = [[0.0; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            result[row][column] = raw[column * 3 + row] as f64;
        }
    }
    result
}

fn component_expression<const N: usize>(basis: &[[f64; N]], component: usize) -> String {
    let terms: Vec<_> = basis
        .iter()
        .enumerate()
        .filter_map(|(parameter, vector)| {
            (vector[component].abs() > EPSILON).then_some((parameter, vector[component]))
        })
        .collect();
    if terms.is_empty() {
        return "0".to_string();
    }

    terms
        .into_iter()
        .enumerate()
        .map(|(term_index, (parameter, coefficient))| {
            let negative = coefficient < 0.0;
            let magnitude = snapped_magnitude(coefficient.abs());
            let symbol = parameter_symbol(parameter);
            let term = if (magnitude - 1.0).abs() < EPSILON {
                symbol.to_string()
            } else {
                format!("{}{}", display_number(magnitude), symbol)
            };
            match (term_index, negative) {
                (0, true) => format!("−{term}"),
                (0, false) => term,
                (_, true) => format!(" − {term}"),
                (_, false) => format!(" + {term}"),
            }
        })
        .collect()
}

fn parameter_symbol(index: usize) -> &'static str {
    const SYMBOLS: [&str; 18] = [
        "α", "β", "γ", "δ", "ε", "ζ", "η", "θ", "ι", "κ", "λ", "μ", "ν", "ξ", "ο", "π", "ρ", "τ",
    ];
    SYMBOLS.get(index).copied().unwrap_or("p")
}

fn snapped_magnitude(value: f64) -> f64 {
    for denominator in 1..=12 {
        let numerator = (value * f64::from(denominator)).round();
        let candidate = numerator / f64::from(denominator);
        if (value - candidate).abs() < 1.0e-6 {
            return candidate;
        }
    }
    value
}

fn display_number(value: f64) -> String {
    let value = snapped_magnitude(value);
    if (value - value.round()).abs() < EPSILON {
        return format!("{:.0}", value);
    }
    let mut result = format!("{value:.3}");
    while result.ends_with('0') {
        result.pop();
    }
    result.trim_end_matches('.').to_string()
}

fn tensor_transform(operation: &CartesianOperation) -> [[f64; 18]; 18] {
    let raw = operation.rotation.to_cols_array();
    // For antiunitary operations k transforms as -Rk, as in Eq. (S131).
    let sign = if operation.time_reversal { -1.0 } else { 1.0 };
    let mut rotation = [[0.0; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            rotation[row][column] = sign * raw[column * 3 + row] as f64;
        }
    }
    let mut transform = [[0.0; 18]; 18];
    for (row_index, &(a, b, c)) in COMPONENTS.iter().enumerate() {
        for (column_index, &(ap, bp, cp)) in COMPONENTS.iter().enumerate() {
            transform[row_index][column_index] = if bp == cp {
                rotation[a][ap] * rotation[b][bp] * rotation[c][cp]
            } else {
                rotation[a][ap]
                    * (rotation[b][bp] * rotation[c][cp] + rotation[b][cp] * rotation[c][bp])
            };
        }
    }
    transform
}

fn append_intrinsic_constraints(rows: &mut Vec<[f64; 18]>, mechanism: TransportMechanism) {
    if mechanism == TransportMechanism::Drude {
        // Eq. (S133): the nonlinear Drude term is fully symmetric.
        for a in 0..3 {
            for b in 0..3 {
                for c in 0..3 {
                    let left = component_index(a, b, c);
                    let right = component_index(b, a, c);
                    if left != right {
                        let mut row = [0.0; 18];
                        row[left] = 1.0;
                        row[right] = -1.0;
                        rows.push(row);
                    }
                }
            }
        }
    }
    if matches!(
        mechanism,
        TransportMechanism::Bcd | TransportMechanism::InterQmd
    ) {
        // Eq. (S134): these contributions are transverse for b = c.
        for a in 0..3 {
            let mut row = [0.0; 18];
            row[component_index(a, a, a)] = 1.0;
            rows.push(row);
        }
    }
    if mechanism == TransportMechanism::Bcd {
        let mut cyclic = [0.0; 18];
        cyclic[component_index(0, 1, 2)] = 1.0;
        cyclic[component_index(1, 2, 0)] = 1.0;
        cyclic[component_index(2, 0, 1)] = 1.0;
        rows.push(cyclic);
        for a in 0..3 {
            for b in 0..3 {
                if a != b {
                    let mut row = [0.0; 18];
                    row[component_index(a, a, b)] = 2.0;
                    row[component_index(b, a, a)] = 1.0;
                    rows.push(row);
                }
            }
        }
    }
}

fn component_index(a: usize, b: usize, c: usize) -> usize {
    let (b, c) = if b <= c { (b, c) } else { (c, b) };
    COMPONENTS
        .iter()
        .position(|&(i, j, k)| (i, j, k) == (a, b, c))
        .expect("valid tensor component")
}

fn nullspace<const N: usize>(mut matrix: Vec<[f64; N]>) -> Vec<[f64; N]> {
    let mut pivot_columns = Vec::new();
    let mut pivot_row = 0;
    for column in 0..N {
        let Some(best_row) = (pivot_row..matrix.len())
            .max_by(|&left, &right| {
                matrix[left][column]
                    .abs()
                    .total_cmp(&matrix[right][column].abs())
            })
            .filter(|&row| matrix[row][column].abs() > EPSILON)
        else {
            continue;
        };
        matrix.swap(pivot_row, best_row);
        let pivot = matrix[pivot_row][column];
        for value in &mut matrix[pivot_row] {
            *value /= pivot;
        }
        for row in 0..matrix.len() {
            if row == pivot_row {
                continue;
            }
            let scale = matrix[row][column];
            if scale.abs() > EPSILON {
                for col in column..N {
                    matrix[row][col] -= scale * matrix[pivot_row][col];
                }
            }
        }
        pivot_columns.push(column);
        pivot_row += 1;
        if pivot_row == matrix.len() {
            break;
        }
    }
    let free_columns: Vec<_> = (0..N)
        .filter(|column| !pivot_columns.contains(column))
        .collect();
    free_columns
        .into_iter()
        .map(|free| {
            let mut vector = [0.0; N];
            vector[free] = 1.0;
            for (row, &pivot) in pivot_columns.iter().enumerate() {
                vector[pivot] = -matrix[row][free];
            }
            vector
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::Vec3;

    #[test]
    fn inversion_forbids_every_polar_third_rank_response() {
        let operations = [CartesianOperation {
            rotation: -Mat3::IDENTITY,
            time_reversal: false,
        }];
        for mechanism in TransportMechanism::ALL {
            assert!(mechanism_basis(&operations, mechanism).is_empty());
        }
    }

    #[test]
    fn identifies_a_grey_magnetic_point_group() {
        let mut operations = Vec::new();
        for x in [-1.0, 1.0] {
            for y in [-1.0, 1.0] {
                for z in [-1.0, 1.0] {
                    let rotation = Mat3::from_diagonal(Vec3::new(x, y, z));
                    operations.push(MagneticOperation {
                        rotation,
                        time_reversal: false,
                    });
                    operations.push(MagneticOperation {
                        rotation,
                        time_reversal: true,
                    });
                }
            }
        }
        assert_eq!(magnetic_group_label(&operations), "mmm.1′");
    }

    #[test]
    fn maps_bns_numbers_to_canonical_magnetic_point_groups() {
        assert_eq!(
            magnetic_point_group_from_bns("52.318").as_deref(),
            Some("mmm.1′")
        );
        assert_eq!(
            magnetic_point_group_from_bns("47.251").as_deref(),
            Some("m′mm")
        );
        assert_eq!(
            magnetic_point_group_from_bns("052.0318").as_deref(),
            Some("mmm.1′")
        );
    }

    #[test]
    fn formats_equal_and_opposite_tensor_components_with_shared_parameters() {
        let mut alpha = [0.0; 18];
        alpha[0] = 1.0;
        alpha[1] = -1.0;
        alpha[2] = -2.0;
        let basis = [alpha];
        assert_eq!(component_expression(&basis, 0), "α");
        assert_eq!(component_expression(&basis, 1), "−α");
        assert_eq!(component_expression(&basis, 2), "−2α");
        assert_eq!(component_expression(&basis, 3), "0");
    }

    #[test]
    fn time_reversal_enforces_onsager_symmetry_for_linear_conductivity() {
        let operations = [CartesianOperation {
            rotation: Mat3::IDENTITY,
            time_reversal: true,
        }];
        let basis = linear_conductivity_basis(&operations);
        assert_eq!(basis.len(), 6);
        for vector in basis {
            for row in 0..3 {
                for column in 0..3 {
                    assert!((vector[row * 3 + column] - vector[column * 3 + row]).abs() < EPSILON);
                }
            }
        }
    }

    #[test]
    fn unconstrained_linear_conductivity_has_nine_components() {
        let operations = [CartesianOperation {
            rotation: Mat3::IDENTITY,
            time_reversal: false,
        }];
        assert_eq!(linear_conductivity_basis(&operations).len(), 9);
    }

    #[test]
    fn matches_nolhallsym_for_mprime_mm() {
        // BNS 47.251 / UNI 349: Pm′mm → m′mm.
        let mut operations = Vec::new();
        for x in [-1.0, 1.0] {
            for y in [-1.0, 1.0] {
                for z in [-1.0, 1.0] {
                    operations.push(CartesianOperation {
                        rotation: Mat3::from_diagonal(Vec3::new(x, y, z)),
                        // m_x is primed; multiplying by it primes precisely
                        // the operations with a negative x component.
                        time_reversal: x < 0.0,
                    });
                }
            }
        }
        let degrees: Vec<_> = TransportMechanism::ALL
            .into_iter()
            .map(|mechanism| mechanism_basis(&operations, mechanism).len())
            .collect();
        assert_eq!(degrees, [3, 0, 4, 5]);
    }

    #[test]
    fn matches_nolhallsym_for_hexagonal_32prime() {
        // BNS 150.27 / UNI 1257: P32′1 → 32′1. This also protects the
        // f32 crystal-to-Cartesian conversion used for hexagonal cells.
        let cell = wyckoscope::model::UnitCell::new(11.48, 11.48, 11.93, 90.0, 90.0, 120.0);
        let operations = [
            ([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], false),
            ([[0.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 0.0, 1.0]], false),
            ([[-1.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], false),
            ([[1.0, -1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]], true),
            ([[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]], true),
            ([[-1.0, 0.0, 0.0], [-1.0, 1.0, 0.0], [0.0, 0.0, -1.0]], true),
        ]
        .map(|(rows, time_reversal)| CartesianOperation {
            rotation: cell.symmetry_to_cartesian(Mat3::from_cols_array(&[
                rows[0][0], rows[1][0], rows[2][0], rows[0][1], rows[1][1], rows[2][1], rows[0][2],
                rows[1][2], rows[2][2],
            ])),
            time_reversal,
        });
        let degrees: Vec<_> = TransportMechanism::ALL
            .into_iter()
            .map(|mechanism| mechanism_basis(&operations, mechanism).len())
            .collect();
        assert_eq!(degrees, [3, 1, 2, 4]);
    }
}
