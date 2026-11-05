//! Anchor assignment (§6.1 `anchor.rs::assign`): after template extraction,
//! one walk assigns each dynamic its precomputed address from the
//! root-relative child-index path recorded during extraction.
//!
//! M0 emits `Down`-only chains (index addressing over slab links);
//! `Next`/`MarkerDown` steps exist in the schema for the elision work that
//! lands with M1 codegen.

use lig_core::id::FragmentId;
use lig_ir::ViewModule;
use lig_ir::anchor::AnchorStep;

/// Fill `module.fragments[frag].dynamics[i].anchor` from the recorded paths.
pub fn assign_anchor_paths(module: &mut ViewModule, frag: FragmentId, paths: &[Vec<u32>]) {
    let Some(f) = module.fragments.get_mut(frag.get() as usize) else {
        return;
    };
    for (di, path) in paths.iter().enumerate() {
        if di >= f.dynamics.len() {
            break;
        }
        let steps: Vec<AnchorStep> = path.iter().map(|&i| AnchorStep::Down(i)).collect();
        f.dynamics[di].anchor = lig_ir::Anchor(steps);
    }
}

#[cfg(test)]
mod tests {
    use super::assign_anchor_paths;
    use lig_core::id::FragmentId;
    use lig_ir::anchor::AnchorStep;
    use lig_ir::fragment::DynamicIr;
    use lig_ir::template::TemplateIr;
    use lig_ir::{DynamicKind, ExprId, FragmentIr, Frontend, ViewModule};

    #[test]
    fn paths_become_down_chains() {
        let mut m = ViewModule::new(Frontend::Solid);
        let name = m.strings.intern("F");
        let tpl = m.intern_template(TemplateIr::new(lig_ir::template::NodeIr::Marker));
        m.fragments.push(FragmentIr {
            name,
            template: tpl,
            dynamics: vec![DynamicIr {
                anchor: lig_ir::Anchor::root(),
                kind: DynamicKind::Text,
                expr: ExprId::new(0),
                reaction: None,
                span: (0, 0),
            }],
            children: vec![],
            props: vec![],
            returns: vec![],
        });
        assign_anchor_paths(&mut m, FragmentId::new(0), &[vec![1, 2]]);
        let steps = m.fragments[0].dynamics[0].anchor.steps();
        assert_eq!(steps, &[AnchorStep::Down(1), AnchorStep::Down(2)]);
    }
}
