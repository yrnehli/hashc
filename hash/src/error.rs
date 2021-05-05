//! Compiler general error reporting
//
// All rights reserved 2021 (c) The Hash Language authors

use std::process::exit;

/// Error message prefix
const ERR: &str = "\x1b[31m\x1b[1merror\x1b[0m";

/// Errors that might occur when attempting to interpret a program
pub enum ErrorType {
    IoError,
    // CicrularDependency,
    InternalError,
}

impl Default for ErrorType {
    fn default() -> Self {
        Self::InternalError
    }
}

/// Function that is used to report a general compiler error
pub fn report_error(err_type: ErrorType, err_msg: String) {
    let prefix = match err_type {
        ErrorType::IoError => "Failed to read file",
        ErrorType::InternalError => "Internal Panic", // @@TODO: add an internal panic function
    };

    println!("{}: {}{}", ERR, prefix, err_msg);
    exit(-1);
}