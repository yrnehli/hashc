//! Hash Compiler pipeline errors that can occur during the
//! the pipeline initialisation.
use std::{io, str::FromStr};

use hash_reporting::report::{Report, ReportKind};
use hash_target::Target;

use crate::settings::{
    CodeGenBackend, CompilerSettings, CompilerStageKind, IrDumpMode, OptimisationLevel,
};

/// Errors that might occur when attempting to compile and or interpret a
/// program.
#[derive(Debug)]
pub enum ArgumentError {
    /// Invalid target was passed to the compiler.
    InvalidTarget(String),

    /// Generic IO error.
    Io(io::Error),

    /// When a specific "stage" of the compiler is specified.
    /// but there exists no such stage.
    UnknownStage(String),

    /// When a stage is specified but the entry point is missing.
    MissingEntryPoint,

    /// When a configuration key is not recognised.
    UnknownKey(String),

    /// When a key-value pair doesn't follow the standard
    /// `-C<key>=<value>` format.
    MalformedKey(String),

    /// A key was provided but was missing a key.
    MissingValue(String),

    /// When a configuration key value is not a valid option
    /// for the specified key.
    InvalidValue(String, String),
}

impl From<ArgumentError> for Report {
    fn from(value: ArgumentError) -> Self {
        let mut report = Report::new();
        let message = match value {
            ArgumentError::InvalidTarget(target) => format!(
                "invalid target `{target}` specified, available targets are: `x86_64` and `x64`"
            ),
            ArgumentError::MissingEntryPoint => "missing entry point".to_string(),
            ArgumentError::UnknownStage(stage) => format!("unknown stage `{stage}`, available stages are: `ast-gen`, `check`, `ir-gen`, `build`"),
            ArgumentError::Io(err) => err.to_string(),
            ArgumentError::UnknownKey(key) => {
                format!("unknown configuration key `{key}`")
            }
            ArgumentError::MalformedKey(key) => {
                format!("malformed configuration key `{key}`")
            }
            ArgumentError::MissingValue(key) => {
                format!("missing value for configuration key `{key}`")
            }
            ArgumentError::InvalidValue(key, value) => {
                format!("invalid value `{value}` for configuration key `{key}`")
            }
        };

        report.kind(ReportKind::Error).title(message);
        report
    }
}

/// This function is used to parse the command line arguments that are
/// passed to the compiler, it will return a [CompilerSettings] struct
/// that contains all of the settings that the compiler should use. If
/// there is an error, this will return an error that can be
/// dealt with by the caller.
pub fn parse_settings_from_args() -> Result<CompilerSettings, ArgumentError> {
    let mut settings = CompilerSettings::default();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        // This is a configuration key that specifies the "key" and then
        // the value in the form of `-C<key>=<value>`
        if arg.starts_with("-C") || arg.starts_with("--") {
            parse_option(&mut settings, &mut args, arg)?;
        } else {
            // This is specifying what kind of a stage the compiler should run
            // the job on whether it is `build`, `check`,
            // `ast-gen`...
            match arg.as_str() {
                "build" => {
                    settings.stage = CompilerStageKind::Full;
                }
                "check" => {
                    settings.stage = CompilerStageKind::Typecheck;
                }
                "ast-gen" => {
                    settings.stage = CompilerStageKind::Parse;
                }
                "ir-gen" => {
                    settings.stage = CompilerStageKind::IrGen;
                }
                _ => {
                    return Err(ArgumentError::UnknownStage(arg));
                }
            };

            // The next argument after this is the input file.
            if let Some(filename) = args.next() {
                settings.entry_point = Some(filename);
            } else {
                return Err(ArgumentError::MissingEntryPoint);
            }
        }
    }

    Ok(settings)
}

/// This function is used to parse a single option that is passed to the
/// compiler. The option can either be in the form of a configuration key
/// `-C<key>=<value>` or a flag `--flag`.
///
/// N.B. This function does not deal with the entry point of the compiler.
fn parse_option(
    settings: &mut CompilerSettings,
    args: &mut impl Iterator<Item = String>,
    arg: String,
) -> Result<(), ArgumentError> {
    // This is a configuration key that specifies the "key" and then
    // the value in the form of `-C<key>=<value>`
    if arg.starts_with("-C") {
        // Check if the key is part of this argument or if there is a
        // separate argument for the value, which we assume to be the
        // next argument.
        if arg.len() == 2 {
            if let Some(arg) = args.next() {
                parse_arg_configuration(settings, arg)?;
            } else {
                return Err(ArgumentError::UnknownKey(arg));
            }
        } else {
            parse_arg_configuration(settings, arg.trim_start_matches("-C").to_string())?;
        }
    } else if arg.starts_with("--") {
        let key = arg.trim_start_matches("--").to_string();

        match key.as_str() {
            "debug" => {
                settings.debug = true;
            }
            "output-metrics" => {
                settings.output_metrics = true;
            }
            _ => {
                return Err(ArgumentError::UnknownKey(arg));
            }
        }
    }

    Ok(())
}

/// This function will parse a single `-C<key>=<value>` argument and apply the
/// specified configuration option to the [CompilerSettings]. Some keys may
/// not have a value, and some keys may specify multiple values with a comma
/// separated list.
fn parse_arg_configuration(
    settings: &mut CompilerSettings,
    arg: String,
) -> Result<(), ArgumentError> {
    // First try and see if we have been provided a key-value pair, if not
    // then we will assume that the key is the argument and the value is
    // `None`.
    let (key, value) = if let Some(split_pos) = arg.find('=') {
        let (key, value) = arg.split_at(split_pos);
        (key.to_string(), Some(value[1..].to_string()))
    } else {
        (arg.clone(), None)
    };

    // When a value is expected from a key, but none is provided, this
    // closure will be used to return an error.
    let expected_value = || ArgumentError::MissingValue(key.clone());

    match key.as_str() {
        "target" => {
            let value = value.ok_or_else(expected_value)?;

            let target = Target::from_string(value.clone())
                .ok_or_else(|| ArgumentError::InvalidTarget(value))?;

            settings.codegen_settings.target_info.set_target(target)
        }
        "optimisation-level" => {
            let value = value.ok_or_else(expected_value)?;
            let opt_level = OptimisationLevel::from_str(value.as_str())?;
            settings.optimisation_level = opt_level;
        }
        "dump" => {
            let value = value.ok_or_else(expected_value)?;

            match value.as_str() {
                "ast" => {
                    settings.ast_settings.dump = true;
                }
                "ir" => {
                    settings.lowering_settings.dump = true;
                }
                _ => {
                    return Err(ArgumentError::InvalidValue(key, value));
                }
            }
        }
        "ir-dump-mode" => {
            let value = value.ok_or_else(expected_value)?;

            match value.as_str() {
                "pretty" => {
                    settings.lowering_settings.dump_mode = IrDumpMode::Pretty;
                }
                "graph" => {
                    settings.lowering_settings.dump_mode = IrDumpMode::Graph;
                }
                _ => {
                    return Err(ArgumentError::InvalidValue(key, value));
                }
            }
        }
        "backend" => {
            let value = value.ok_or_else(expected_value)?;

            match value.as_str() {
                "llvm" => {
                    settings.codegen_settings.backend = CodeGenBackend::LLVM;
                }
                "vm" => {
                    settings.codegen_settings.backend = CodeGenBackend::VM;
                }
                _ => {
                    return Err(ArgumentError::InvalidValue(key, value));
                }
            }
        }
        _ => {
            return Err(ArgumentError::UnknownKey(key));
        }
    };

    Ok(())
}
