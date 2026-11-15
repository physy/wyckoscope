mod cif;
mod vesta;

use std::path::Path;

use thiserror::Error;

use crate::model::CrystalStructure;

#[derive(Debug, Clone)]
pub struct StructureCandidate {
    pub data_name: String,
    pub structure: CrystalStructure,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("unsupported file format: {0}")]
    UnsupportedFormat(String),
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("invalid number for {field}: {value}")]
    InvalidNumber { field: &'static str, value: String },
    #[error("no atom sites were found")]
    NoAtoms,
    #[error("invalid structure: {0}")]
    InvalidStructure(String),
    #[error("could not read file: {0}")]
    Io(#[from] std::io::Error),
}

pub fn parse_path(path: &Path) -> Result<CrystalStructure, ParseError> {
    parse_path_candidates(path).map(|mut candidates| candidates.remove(0).structure)
}

pub fn parse_path_candidates(path: &Path) -> Result<Vec<StructureCandidate>, ParseError> {
    let text = std::fs::read_to_string(path)?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    parse_text_candidates(&text, &extension)
}

pub fn parse_text(text: &str, extension: &str) -> Result<CrystalStructure, ParseError> {
    parse_text_candidates(text, extension).map(|mut candidates| candidates.remove(0).structure)
}

pub fn parse_text_candidates(
    text: &str,
    extension: &str,
) -> Result<Vec<StructureCandidate>, ParseError> {
    match extension
        .trim_start_matches('.')
        .to_ascii_lowercase()
        .as_str()
    {
        "cif" | "mcif" => cif::parse_candidates(text),
        "vesta" => Ok(vec![StructureCandidate {
            data_name: String::new(),
            structure: vesta::parse(text)?,
        }]),
        other => Err(ParseError::UnsupportedFormat(other.to_string())),
    }
}

pub(crate) fn parse_number(value: &str, field: &'static str) -> Result<f32, ParseError> {
    let cleaned = value.trim().trim_matches(['\'', '"']);
    let without_uncertainty = if let Some(index) = cleaned.find('(') {
        &cleaned[..index]
    } else {
        cleaned
    };
    if let Some((numerator, denominator)) = without_uncertainty.split_once('/') {
        let a = numerator
            .parse::<f32>()
            .map_err(|_| ParseError::InvalidNumber {
                field,
                value: value.to_string(),
            })?;
        let b = denominator
            .parse::<f32>()
            .map_err(|_| ParseError::InvalidNumber {
                field,
                value: value.to_string(),
            })?;
        if b.abs() < f32::EPSILON {
            return Err(ParseError::InvalidNumber {
                field,
                value: value.to_string(),
            });
        }
        return Ok(a / b);
    }
    without_uncertainty
        .parse::<f32>()
        .map_err(|_| ParseError::InvalidNumber {
            field,
            value: value.to_string(),
        })
}
