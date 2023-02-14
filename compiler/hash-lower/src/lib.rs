//! Hash Intermediate Representation builder. This crate contains the
//! functionality that converts the Hash typed AST into Hash IR. Additionally,
//! the Hash IR builder crate contains implemented passes that will optimise the
//! IR, performing optimisations such as constant folding or dead code
//! elimination.
#![feature(decl_macro, let_chains, never_type, unwrap_infallible)]

mod build;
mod cfg;
// mod discover;
mod new_discover;
mod optimise;
mod ty;

use build::{Builder, Tcx};
use hash_ir::{
    write::{graphviz, pretty},
    IrStorage,
};
use hash_layout::{compute::LayoutComputer, write::LayoutWriter, LayoutCtx, TyInfo};
use hash_pipeline::{
    interface::{CompilerInterface, CompilerOutputStream, CompilerResult, CompilerStage},
    settings::{CompilerSettings, CompilerStageKind, IrDumpMode},
    workspace::{SourceStageInfo, Workspace},
};
use hash_semantics::SemanticStorage;
use hash_source::{identifier::IDENTS, location::SourceLocation, SourceId};
use hash_tir::{
    data::DataTy,
    environment::{
        env::{AccessToEnv, Env},
        source_info::CurrentSourceInfo,
    },
    utils::common::CommonUtils,
};
use hash_utils::{store::Store, stream_writeln};
use new_discover::FnDiscoverer;
use optimise::Optimiser;
use ty::TyLoweringCtx;

/// The Hash IR builder compiler stage. This will walk the AST, and
/// lower all items within a particular module.
#[derive(Default)]
pub struct IrGen {
    /// When the visitor is walking modules, it looks for `#layout_of`
    /// directives on type declarations. This is a collection of all of the
    /// type definitions that were found and require a layout to be
    /// generated.
    layouts_to_generate: Vec<DataTy>,
}

/// The [LoweringCtx] represents all of the required information
/// that the [IrGen] stage needs to query from the pipeline
/// in order to perform its lowering operations.
pub struct LoweringCtx<'ir> {
    /// Reference to the current compiler workspace.
    pub workspace: &'ir mut Workspace,

    /// The settings of the current session.
    pub settings: &'ir CompilerSettings,

    /// Reference to the semantic storage that comes from
    /// the typechecking compiler phase.
    pub semantic_storage: &'ir SemanticStorage,

    /// Reference to the IR storage that is used to store
    /// the lowered IR, and all metadata about the IR.
    pub ir_storage: &'ir mut IrStorage,

    /// Reference to the [LayoutCtx] that is used to store
    /// the layouts of types.
    pub layout_storage: &'ir LayoutCtx,

    /// Reference to the output stream
    pub stdout: CompilerOutputStream,

    /// Reference to the rayon thread pool.
    pub _pool: &'ir rayon::ThreadPool,
}

pub trait LoweringCtxQuery: CompilerInterface {
    fn data(&mut self) -> LoweringCtx<'_>;
}

impl<Ctx: LoweringCtxQuery> CompilerStage<Ctx> for IrGen {
    /// Return that this is [CompilerStageKind::Lower].
    fn kind(&self) -> CompilerStageKind {
        CompilerStageKind::Lower
    }

    /// Lower that AST of each module that is currently in the workspace
    /// into Hash IR. This will iterate over all modules, and possibly
    /// interactive statements to see if the need IR lowering, if so they
    /// are lowered and the result is saved on the [IrStorage].
    /// Additionally, this module is responsible for performing
    /// optimisations on the IR (if specified via the [CompilerSettings]).
    fn run(&mut self, entry: SourceId, ctx: &mut Ctx) -> CompilerResult<()> {
        let LoweringCtx { semantic_storage, workspace, ir_storage, settings, .. } = ctx.data();
        let source_stage_info = &mut workspace.source_stage_info;

        let mut lowered_bodies = Vec::new();

        let source_info = CurrentSourceInfo { source_id: entry };
        let env = Env::new(
            &semantic_storage.stores,
            &semantic_storage.context,
            &workspace.node_map,
            &workspace.source_map,
            &source_info,
        );
        let discoverer = FnDiscoverer::new(&env);

        for func in discoverer.discover_fns().iter() {
            let symbol = discoverer.stores().fn_def().map_fast(*func, |func| func.name);
            let name = discoverer
                .stores()
                .symbol()
                .map_fast(symbol, |symbol| symbol.name.unwrap_or(IDENTS.underscore));

            // Get the source of the symbol therefore that way
            // we can get the source id of the function.
            let Some(SourceLocation { id, .. }) = discoverer.get_location(symbol) else {
                panic!("function has no defined source location");
            };

            let primitives = match semantic_storage.primitives_or_unset.get() {
                Some(primitives) => primitives,
                None => panic!("Tried to get primitives but they are not set yet"),
            };

            let tcx = Tcx { env: &env, primitives };
            let mut builder =
                Builder::new(name, (*func).into(), id, tcx, &mut ir_storage.ctx, settings);
            builder.build();

            // add the body to the lowered bodies
            lowered_bodies.push(builder.finish());
            //@@Todo: we need to check if this item is marked to be dumped...
        }

        // @@Todo: deal with the entry point here.

        //     if let Some(instance) = discoverer.entry_point_instance() {
        //         let kind = ty_storage.entry_point_state.kind().unwrap();
        //         ir_storage.entry_point.set(instance, kind);
        //     }

        // Mark all modules now as lowered, and all generated
        // bodies to the store.
        source_stage_info.set_all(SourceStageInfo::LOWERED);
        ir_storage.add_bodies(lowered_bodies);

        Ok(())
    }

    fn cleanup(&mut self, entry: SourceId, stage_data: &mut Ctx) {
        let LoweringCtx {
            semantic_storage, ir_storage, layout_storage, workspace, mut stdout, ..
        } = stage_data.data();
        let source_info = CurrentSourceInfo { source_id: entry };
        let env = Env::new(
            &semantic_storage.stores,
            &semantic_storage.context,
            &workspace.node_map,
            &workspace.source_map,
            &source_info,
        );

        let ty_lowerer = TyLoweringCtx::new(&ir_storage.ctx, &env);

        // @@Todo: use terms instead of ast-nodes...?
        for (index, type_def) in self.layouts_to_generate.iter().enumerate() {
            // fetch or compute the type of the type definition.
            let ty = ty_lowerer.ty_id_from_tir_data(*type_def);
            let layout_computer = LayoutComputer::new(layout_storage, &ir_storage.ctx);

            // @@ErrorHandling: propagate this error if it occurs.
            let layout = layout_computer.layout_of_ty(ty).unwrap();

            // Print the layout and add spacing between all of the specified layouts
            // that were requested.
            stream_writeln!(
                stdout,
                "{}",
                LayoutWriter::new(TyInfo { ty, layout }, layout_computer)
            );

            if index < self.layouts_to_generate.len() - 1 {
                stream_writeln!(stdout);
            }
        }

        // Now that we have generated and printed all of the requested
        // layouts for the current session, we can clear the list of
        // layouts to generate.
        self.layouts_to_generate.clear();
    }
}

/// Compiler stage that is responsible for performing optimisations on the
/// Hash IR. This will iterate over all of the bodies that have been generated
/// and perform optimisations on them based on if they are applicable and the
/// current configuration settings of the compiler.
#[derive(Default)]
pub struct IrOptimiser;

impl<Ctx: LoweringCtxQuery> CompilerStage<Ctx> for IrOptimiser {
    /// Return that this is [CompilerStageKind::Lower].
    fn kind(&self) -> CompilerStageKind {
        CompilerStageKind::Lower
    }

    fn run(&mut self, _: SourceId, ctx: &mut Ctx) -> CompilerResult<()> {
        let LoweringCtx { workspace, ir_storage, settings, .. } = ctx.data();
        let source_map = &mut workspace.source_map;

        let bodies = &mut ir_storage.bodies;
        let body_data = &ir_storage.ctx;
        // let optimiser = Optimiser::new(ir_storage, source_map, settings);

        // @@Todo: think about making optimisation passes in parallel...
        // pool.scope(|scope| {
        //     for body in &mut ir_storage.generated_bodies {
        //         scope.spawn(|_| {
        //             optimiser.optimise(body);
        //         });
        //     }
        // });

        for body in bodies.iter_mut() {
            let optimiser = Optimiser::new(body_data, source_map, settings);
            optimiser.optimise(body);
        }

        Ok(())
    }

    fn cleanup(&mut self, _entry_point: SourceId, ctx: &mut Ctx) {
        let settings = ctx.settings().lowering_settings;
        let LoweringCtx { workspace, ir_storage, mut stdout, .. } = ctx.data();
        let source_map = &mut workspace.source_map;
        let bcx = &ir_storage.ctx;

        // we need to check if any of the bodies have been marked for `dumping`
        // and emit the IR that they have generated.
        if settings.dump_mode == IrDumpMode::Graph {
            graphviz::dump_ir_bodies(bcx, &ir_storage.bodies, settings.dump, &mut stdout).unwrap();
        } else {
            pretty::dump_ir_bodies(bcx, source_map, &ir_storage.bodies, settings.dump, &mut stdout)
                .unwrap();
        }
    }
}
