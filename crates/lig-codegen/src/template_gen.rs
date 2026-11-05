//! Template emission (C1): `NodeIr` trees → const `TplNode` slices, deduped
//! per template id; anchors → const `AnchorStep` slice tables.

use lig_ir::ViewModule;
use lig_ir::anchor::AnchorStep as IrStep;
use lig_ir::template::{AttrValue, NodeIr};
use quote::quote;

/// Emit `pub static {prefix}TPL_T{n}: &[TplNode] = &[ ... ];` for one template.
#[must_use]
pub fn gen_template_static(
    module: &ViewModule,
    tpl_id: usize,
    prefix: &str,
) -> proc_macro2::TokenStream {
    let name = syn::Ident::new(
        &format!("{prefix}TPL_T{tpl_id}"),
        proc_macro2::Span::call_site(),
    );
    let Some(tpl) = module.templates.get(tpl_id) else {
        return quote! {};
    };
    let roots = tpl.root.as_single_root().into_iter().collect::<Vec<_>>();
    let nodes = gen_nodes(module, &roots);
    quote! {
        pub static #name: &[::lig_runtime::TplNode] = &[ #(#nodes),* ];
    }
}

/// Emit `static ANCHORS_F{n}: &[&[AnchorStep]] = &[ ... ];` plus the branch
/// variant.
#[must_use]
pub fn gen_anchor_statics(
    frag_id: usize,
    hole_paths: &[Vec<IrStep>],
    branch_paths: &[Vec<IrStep>],
    prefix: &str,
) -> proc_macro2::TokenStream {
    let hole_name = syn::Ident::new(
        &format!("{prefix}HOLE_ANCHORS_F{frag_id}"),
        proc_macro2::Span::call_site(),
    );
    let branch_name = syn::Ident::new(
        &format!("{prefix}BRANCH_ANCHORS_F{frag_id}"),
        proc_macro2::Span::call_site(),
    );
    let holes = hole_paths.iter().map(|p| gen_anchor(p));
    let branches = branch_paths.iter().map(|p| gen_anchor(p));
    quote! {
        static #hole_name: &[&[::lig_runtime::AnchorStep]] = &[ #(#holes),* ];
        static #branch_name: &[&[::lig_runtime::AnchorStep]] = &[ #(#branches),* ];
    }
}

fn gen_anchor(path: &[IrStep]) -> proc_macro2::TokenStream {
    let steps = path.iter().map(|s| match s {
        IrStep::Down(i) => {
            let i = *i;
            quote!(::lig_runtime::AnchorStep::Down(#i))
        }
        IrStep::Next => quote!(::lig_runtime::AnchorStep::Next),
        IrStep::MarkerDown(i) => {
            let i = *i;
            quote!(::lig_runtime::AnchorStep::MarkerDown(#i))
        }
    });
    quote!(&[ #(#steps),* ])
}

fn gen_nodes(module: &ViewModule, nodes: &[NodeIr]) -> Vec<proc_macro2::TokenStream> {
    nodes.iter().map(|n| gen_node(module, n)).collect()
}

fn gen_node(module: &ViewModule, node: &NodeIr) -> proc_macro2::TokenStream {
    match node {
        NodeIr::Element {
            tag,
            static_attrs,
            style,
            children,
        } => {
            let tag = module.strings.get(*tag);
            let tag_lit = syn::LitStr::new(tag, proc_macro2::Span::call_site());
            let attrs = static_attrs.iter().map(|a| {
                let key = module.strings.get(a.key);
                let key_lit = syn::LitStr::new(key, proc_macro2::Span::call_site());
                let val = match &a.value {
                    AttrValue::Str(s) => {
                        let v = module.strings.get(*s);
                        let v_lit = syn::LitStr::new(v, proc_macro2::Span::call_site());
                        quote!(::lig_runtime::AttrVal::Str(#v_lit))
                    }
                    AttrValue::Bool(b) => quote!(::lig_runtime::AttrVal::Bool(#b)),
                    AttrValue::Num(n) => quote!(::lig_runtime::AttrVal::Num(#n)),
                };
                quote!(::lig_runtime::TplAttr { key: #key_lit, val: #val })
            });
            let style = if let Some(s) = style {
                let id = s.get();
                quote!(Some(::lig_runtime::StyleConstId(#id)))
            } else {
                quote!(None)
            };
            let children = gen_nodes(module, children);
            quote! {
                ::lig_runtime::TplNode::Element {
                    tag: #tag_lit,
                    attrs: &[ #(#attrs),* ],
                    style: #style,
                    children: &[ #(#children),* ],
                }
            }
        }
        NodeIr::Text { value } => {
            let v = module.strings.get(*value);
            let v_lit = syn::LitStr::new(v, proc_macro2::Span::call_site());
            quote!(::lig_runtime::TplNode::Text { value: #v_lit })
        }
        NodeIr::Marker => quote!(::lig_runtime::TplNode::Marker),
    }
}

/// Emit the style table: `pub static {prefix}STYLE_CSS: &[&str] = &[ ... ];`.
#[must_use]
pub fn gen_style_table(module: &ViewModule, prefix: &str) -> proc_macro2::TokenStream {
    let entries = module.styles.iter().map(|s| {
        let css = syn::LitStr::new(&s.css, proc_macro2::Span::call_site());
        quote!(#css)
    });
    let name = syn::Ident::new(
        &format!("{prefix}STYLE_CSS"),
        proc_macro2::Span::call_site(),
    );
    quote! {
        pub static #name: &[&str] = &[ #(#entries),* ];
    }
}

/// Helper trait bridging the IR's single-root template shape.
trait AsSingleRoot {
    fn as_single_root(&self) -> Option<NodeIr>;
}

impl AsSingleRoot for NodeIr {
    fn as_single_root(&self) -> Option<NodeIr> {
        Some(self.clone())
    }
}
