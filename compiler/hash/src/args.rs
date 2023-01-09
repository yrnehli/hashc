//! Hash Compiler arguments management.

use clap::{self, command, ArgAction, Parser, Subcommand};
use hash_pipeline::settings::{
    AstSettings, CodeGenSettings, CompilerSettings, CompilerStageKind, IrDumpMode,
    LoweringSettings, OptimisationLevel,
};
use hash_reporting::errors::CompilerError;
use hash_target::{Target, TargetInfo};

/// CompilerOptions is a structural representation of what arguments the
/// compiler can take when running. Compiler options are well documented on the
/// wiki page: <https://hash-org.github.io/hash-arxiv/interpreter-options.html>
//
// @@Todo: we want to introduce a new option for the compiler which allows one to
//         specify arbitrary configuration options to the compiler. Since there
//         are always new options being added to each *stage*, we want to be able to
//         access all of the options from any stage. The format of these flags should
//         be the following:
//         ```text
//         -C<stage>-<key> (=<value>)?
//         ```
//         This means that the user can specify flags for for particular stages using this
//         format.
#[derive(Parser)]
#[command(
    name = "Hash Interpreter",
    version,
    author = "Hash Language Authors",
    about = "Run and execute hash programs",
    disable_colored_help = true
)]
pub(crate) struct CompilerOptions {
    /// Execute the passed script directly without launching interactive mode
    #[arg(short, long)]
    pub(crate) filename: Option<String>,

    /// Run the compiler in debug mode. This does not affect the optimisation
    /// level of the compiler.
    #[arg(long)]
    pub(crate) debug: bool,

    /// The optimisation level that the compiler should run at. This can be
    /// specified as a either `debug`, or `release`.
    #[arg(long, value_enum, default_value = "debug")]
    pub(crate) optimisation_level: OptimisationLevel,

    /// Set the maximum stack size for the current running instance.
    //
    // @@Todo: move this into `VmOptions` config rather than being
    // directly on the compiler options.
    #[arg(long, default_value = "10000")]
    pub(crate) stack_size: usize,

    /// Whether to dump the generated ast
    #[arg(long)]
    pub(crate) dump_ast: bool,

    /// Whether to output the result of each compiler stage. This flag
    /// supersedes `--dump-ast` when they are both enabled.
    #[arg(long)]
    pub(crate) output_stage_results: bool,

    /// Whether to print the stage metrics for each stage of the compiler.
    #[arg(long)]
    pub(crate) output_metrics: bool,

    /// Number of worker threads the compiler should use
    #[arg(short, long, default_value_t = num_cpus::get())]
    pub(crate) worker_count: usize,

    /// The target that the compiler will emit the executable for. This
    /// will be used to determine the pointer size and other settings that
    /// are **target specific**.
    #[arg(long, default_value = std::env::consts::ARCH)]
    pub(crate) target: String,

    /// Compiler mode
    #[command(subcommand)]
    pub(crate) mode: Option<SubCmd>,
}

/// [SubCmd] specifies separate modes that the compiler can run in. These modes
/// are used to terminate the compiler at a particular stage of the pipeline.
#[derive(Subcommand, Clone)]
pub(crate) enum SubCmd {
    /// Parse the given program and terminate.
    AstGen(AstGenMode),
    /// Only run the compiler up until the `de-sugaring` stage.
    DeSugar(DeSugarMode),
    /// Typecheck the given module.
    Check(CheckMode),
    /// Generate the IR for the given file.
    IrGen(IrGenMode),
}

/// Desugar from given input file
#[derive(Parser, Clone)]
pub(crate) struct DeSugarMode {
    /// Input filename of the module
    #[arg(required = true)]
    pub(crate) filename: String,
}

/// Generate AST from given input file
#[derive(Parser, Clone)]
pub(crate) struct AstGenMode {
    /// Input filename of the module
    #[arg(required = true)]
    pub(crate) filename: String,
}

/// Typecheck the provided module
#[derive(Parser, Clone)]
pub(crate) struct CheckMode {
    /// Input filename of the module
    #[arg(required = true)]
    pub(crate) filename: String,
}

/// Generate IR from the given input file
#[derive(Parser, Clone)]
pub(crate) struct IrGenMode {
    /// Input filename of the module
    #[arg(required = true)]
    pub(crate) filename: String,

    /// If the IR should be printed to stdout or not, and in which
    /// format, options are `pretty` or `graph`.
    #[arg(long, value_enum, default_value_t = IrDumpMode::Pretty)]
    pub(crate) dump_mode: IrDumpMode,

    /// Whether to print the IR to stdout or not.
    #[arg(long, default_value_t = false)]
    pub(crate) dump: bool,

    /// Whether the IR should use `checked` operations, if this flag
    /// is specified, this will insert `checked` operations regardless
    /// of the optimisation level.
    #[arg(long, default_value_t = true, action = ArgAction::Set)]
    pub(crate) checked_operations: bool,
}

impl TryInto<CompilerSettings> for CompilerOptions {
    type Error = CompilerError;

    fn try_into(self) -> Result<CompilerSettings, Self::Error> {
        let mut lowering_settings = LoweringSettings::default();
        let ast_settings = AstSettings { dump_tree: self.dump_ast };

        let stage = match self.mode {
            Some(SubCmd::AstGen { .. }) => CompilerStageKind::Parse,
            Some(SubCmd::DeSugar { .. }) => CompilerStageKind::DeSugar,

            Some(SubCmd::Check { .. }) => CompilerStageKind::Typecheck,
            Some(SubCmd::IrGen(opts)) => {
                let checked_operations = if self.optimisation_level == OptimisationLevel::Release {
                    false
                } else {
                    opts.checked_operations
                };

                // @@Todo: this should be configurable outside of the
                // "ir-gen" mode.
                lowering_settings = LoweringSettings {
                    dump_mode: opts.dump_mode,
                    dump_all: opts.dump,
                    checked_operations,
                };

                CompilerStageKind::IrGen
            }
            _ => CompilerStageKind::Full,
        };

        // We can use the default value of target since we are running
        // on the current system...
        let host = Target::default();

        let target = Target::from_string(self.target.clone())
            .ok_or_else(|| CompilerError::InvalidTarget(self.target))?;

        let target_info = TargetInfo::new(host, target);

        let codegen_settings = CodeGenSettings { target_info, ..CodeGenSettings::default() };

        Ok(CompilerSettings {
            ast_settings,
            lowering_settings,
            codegen_settings,
            optimisation_level: self.optimisation_level,
            output_stage_results: self.output_stage_results,
            output_metrics: self.output_metrics,
            worker_count: self.worker_count,
            skip_prelude: false,
            emit_errors: true,
            stage,
        })
    }
}
