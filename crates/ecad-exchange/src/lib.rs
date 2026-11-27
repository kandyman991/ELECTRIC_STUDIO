//! Exchange with other tools: the PLC programming side, and manufacturers' parts lists.
//!
//! [`parts`]: parts lists from CSV and Excel, for the components library and catalogues.
//!
//! [`tia`]: the TIA Portal PLC tag table. The schematic's PLC channels go out as tags (name,
//! data type, address, comment) that TIA Portal imports; symbols and comments renamed in TIA
//! Portal come back by address.

pub mod csv;
pub mod parts;
pub mod tia;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExchangeError {
    #[error("can't write the tag table: {0}")]
    Write(String),
    #[error("can't read the tag table: {0}")]
    Read(String),
    #[error("can't read the parts list: {0}")]
    Parts(String),
    #[error("the tag table has no `{0}` column")]
    MissingColumn(&'static str),
    #[error("two channels share an address: {}", .0.join("; "))]
    SharedAddresses(Vec<String>),
    #[error(transparent)]
    Model(#[from] ecad_model::ModelError),
}

pub type Result<T> = std::result::Result<T, ExchangeError>;
