//! On-device photographic keyword suggestions, captions and OCR.
mod florence;
mod job;
mod keywords;
pub use job::{ImageUnderstanding, UnderstandingJob, UnderstandingModel, decode_image};
mod mapping;
pub use florence::{Caption, FLORENCE_REVISION, FLORENCE_VERSION, Florence, parse_ocr};
pub use keywords::{Calibration, KeywordModel, rank_keywords, vocabulary};
pub use mapping::{MappedKeyword, WritePolicy, accept_keywords, map_keyword};
