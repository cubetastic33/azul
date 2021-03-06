//! DOM-tree to CSS style tree stying

use std::{fmt, collections::BTreeMap};
use azul_css::{
    Css, CssContentGroup, CssDeclaration, CssPath,
    CssPathSelector, CssPathPseudoSelector, CssNthChildSelector::*,
};
use webrender::api::HitTestItem;
use {
    traits::Layout,
    ui_description::{UiDescription, StyledNode},
    dom::NodeData,
    ui_state::UiState,
    id_tree::{NodeId, NodeHierarchy, NodeDataContainer},
    callbacks::FocusTarget,
};

/// Has all the necessary information about the style CSS path
pub(crate) struct HtmlCascadeInfo<'a, T: 'a + Layout> {
    pub node_data: &'a NodeData<T>,
    pub index_in_parent: usize,
    pub is_last_child: bool,
    pub is_hovered_over: bool,
    pub is_focused: bool,
    pub is_active: bool,
}

impl<'a, T: 'a + Layout> fmt::Debug for HtmlCascadeInfo<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "HtmlCascadeInfo {{ \
            node_data: {:?}, \
            index_in_parent: {}, \
            is_last_child: {:?}, \
            is_hovered_over: {:?}, \
            is_focused: {:?}, \
            is_active: {:?}, \
         }}",
            self.node_data,
            self.index_in_parent,
            self.is_last_child,
            self.is_hovered_over,
            self.is_focused,
            self.is_active,
         )
    }
}

#[test]
fn test_case_issue_93() {

    use azul_css::CssPathSelector::*;
    use azul_css::*;
    use prelude::*;

    struct DataModel { }
    impl Layout for DataModel { fn layout(&self) -> Dom<DataModel> { Dom::div() } }

    fn render_tab() -> Dom<DataModel> {
        Dom::div().with_class("tabwidget-tab")
            .with_child(Dom::label("").with_class("tabwidget-tab-label"))
            .with_child(Dom::label("").with_class("tabwidget-tab-close"))
    }

    let dom = Dom::div().with_id("editor-rooms")
    .with_child(
        Dom::div().with_class("tabwidget-bar")
        .with_child(render_tab().with_class("active"))
        .with_child(render_tab())
        .with_child(render_tab())
        .with_child(render_tab())
    );

    let tab_active_label_default = CssPath { selectors: vec![Class("tabwidget-tab-label".into())] };
    let tab_active_label = CssPath { selectors: vec![Class("tabwidget-tab".into()), Class("active".into()), Children, Class("tabwidget-tab-label".into())] };
    let tab_active_close = CssPath { selectors: vec![Class("tabwidget-tab".into()), Class("active".into()), Children, Class("tabwidget-tab-close".into())] };

    let node_hierarchy = &dom.arena.node_layout;
    let nodes_sorted = node_hierarchy.get_parents_sorted_by_depth();
    let html_node_tree = construct_html_cascade_tree(
        &dom.arena.node_data,
        &node_hierarchy,
        &nodes_sorted,
        None,
        &BTreeMap::new(),
        false,
    );

    //  rules: [
    //    ".tabwidget-tab-label"                        : ColorU::BLACK,
    //    ".tabwidget-tab.active .tabwidget-tab-label"  : ColorU::WHITE,
    //    ".tabwidget-tab.active .tabwidget-tab-close"  : ColorU::RED,
    //  ]

    //  0: [div #editor-rooms ]
    //   |-- 1: [div  .tabwidget-bar]
    //   |    |-- 2: [div  .tabwidget-tab .active]
    //   |    |    |-- 3: [p  .tabwidget-tab-label]
    //   |    |    |-- 4: [p  .tabwidget-tab-close]
    //   |    |-- 5: [div  .tabwidget-tab]
    //   |    |    |-- 6: [p  .tabwidget-tab-label]
    //   |    |    |-- 7: [p  .tabwidget-tab-close]
    //   |    |-- 8: [div  .tabwidget-tab]
    //   |    |    |-- 9: [p  .tabwidget-tab-label]
    //   |    |    |-- 10: [p  .tabwidget-tab-close]
    //   |    |-- 11: [div  .tabwidget-tab]
    //   |    |    |-- 12: [p  .tabwidget-tab-label]
    //   |    |    |-- 13: [p  .tabwidget-tab-close]

    // Test 1:
    // ".tabwidget-tab.active .tabwidget-tab-label"
    // should not match
    // ".tabwidget-tab.active .tabwidget-tab-close"
    assert_eq!(matches_html_element(&tab_active_close, NodeId::new(3), &node_hierarchy, &html_node_tree), false);

    // Test 2:
    // ".tabwidget-tab.active .tabwidget-tab-close"
    // should match
    // ".tabwidget-tab.active .tabwidget-tab-close"
    assert_eq!(matches_html_element(&tab_active_close, NodeId::new(4), &node_hierarchy, &html_node_tree), true);
}

/// Returns if the style CSS path matches the DOM node (i.e. if the DOM node should be styled by that element)
pub(crate) fn matches_html_element<'a, T: Layout>(
    css_path: &CssPath,
    node_id: NodeId,
    node_hierarchy: &NodeHierarchy,
    html_node_tree: &NodeDataContainer<HtmlCascadeInfo<'a, T>>)
-> bool
{
    use self::CssGroupSplitReason::*;

    if css_path.selectors.is_empty() {
        return false;
    }

    let mut current_node = Some(node_id);
    let mut direct_parent_has_to_match = false;
    let mut last_selector_matched = true;

    for (content_group, reason) in CssGroupIterator::new(&css_path.selectors) {
        let cur_node_id = match current_node {
            Some(c) => c,
            None => {
                // The node has no parent, but the CSS path
                // still has an extra limitation - only valid if the
                // next content group is a "*" element
                return *content_group == [&CssPathSelector::Global];
            },
        };
        let current_selector_matches = selector_group_matches(&content_group, &html_node_tree[cur_node_id]);

        if direct_parent_has_to_match && !current_selector_matches {
            // If the element was a ">" element and the current,
            // direct parent does not match, return false
            return false; // not executed (maybe this is the bug)
        }

        // If the current selector matches, but the previous one didn't,
        // that means that the CSS path chain is broken and therefore doesn't match the element
        if current_selector_matches && !last_selector_matched {
            return false;
        }

        // Important: Set if the current selector has matched the element
        last_selector_matched = current_selector_matches;
        // Select if the next content group has to exactly match or if it can potentially be skipped
        direct_parent_has_to_match = reason == DirectChildren;
        current_node = node_hierarchy[cur_node_id].parent;
    }

    last_selector_matched
}

struct CssGroupIterator<'a> {
    pub css_path: &'a Vec<CssPathSelector>,
    pub current_idx: usize,
    pub last_reason: CssGroupSplitReason,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum CssGroupSplitReason {
    Children,
    DirectChildren,
}

impl<'a> CssGroupIterator<'a> {
    pub fn new(css_path: &'a Vec<CssPathSelector>) -> Self {
        let initial_len = css_path.len();
        Self {
            css_path,
            current_idx: initial_len,
            last_reason: CssGroupSplitReason::Children,
        }
    }
}

impl<'a> Iterator for CssGroupIterator<'a> {
    type Item = (CssContentGroup<'a>, CssGroupSplitReason);

    fn next(&mut self) -> Option<(CssContentGroup<'a>, CssGroupSplitReason)> {
        use self::CssPathSelector::*;

        let mut new_idx = self.current_idx;

        if new_idx == 0 {
            return None;
        }

        let mut current_path = Vec::new();

        while new_idx != 0 {
            match self.css_path.get(new_idx - 1)? {
                Children => {
                    self.last_reason = CssGroupSplitReason::Children;
                    break;
                },
                DirectChildren => {
                    self.last_reason = CssGroupSplitReason::DirectChildren;
                    break;
                },
                other => current_path.push(other),
            }
            new_idx -= 1;
        }

        // NOTE: Order inside of a ContentGroup is not important
        // for matching elements, only important for testing
        #[cfg(test)]
        current_path.reverse();

        if new_idx == 0 {
            if current_path.is_empty() {
                None
            } else {
                // Last element of path
                self.current_idx = 0;
                Some((current_path, self.last_reason))
            }
        } else {
            // skip the "Children | DirectChildren" element itself
            self.current_idx = new_idx - 1;
            Some((current_path, self.last_reason))
        }
    }
}

#[test]
fn test_css_group_iterator() {

    use self::CssPathSelector::*;
    use azul_css::NodeTypePath;

    // ".hello > #id_text.new_class div.content"
    // -> ["div.content", "#id_text.new_class", ".hello"]
    let selectors = vec![
        Class("hello".into()),
        DirectChildren,
        Id("id_test".into()),
        Class("new_class".into()),
        Children,
        Type(NodeTypePath::Div),
        Class("content".into()),
    ];

    let mut it = CssGroupIterator::new(&selectors);

    assert_eq!(it.next(), Some((vec![
       &Type(NodeTypePath::Div),
       &Class("content".into()),
    ], CssGroupSplitReason::Children)));

    assert_eq!(it.next(), Some((vec![
       &Id("id_test".into()),
       &Class("new_class".into()),
    ], CssGroupSplitReason::DirectChildren)));

    assert_eq!(it.next(), Some((vec![
        &Class("hello".into()),
    ], CssGroupSplitReason::DirectChildren))); // technically not correct

    assert_eq!(it.next(), None);

    // Test single class
    let selectors_2 = vec![
        Class("content".into()),
    ];

    let mut it = CssGroupIterator::new(&selectors_2);

    assert_eq!(it.next(), Some((vec![
       &Class("content".into()),
    ], CssGroupSplitReason::Children)));

    assert_eq!(it.next(), None);
}

pub(crate) fn construct_html_cascade_tree<'a, T: Layout>(
    input: &'a NodeDataContainer<NodeData<T>>,
    node_hierarchy: &NodeHierarchy,
    node_depths_sorted: &[(usize, NodeId)],
    focused_item: Option<NodeId>,
    hovered_items: &BTreeMap<NodeId, HitTestItem>,
    is_mouse_down: bool
)-> NodeDataContainer<HtmlCascadeInfo<'a, T>>
{
    let mut nodes = (0..node_hierarchy.len()).map(|_| HtmlCascadeInfo {
        node_data: &input[NodeId::new(0)],
        index_in_parent: 0,
        is_last_child: false,
        is_hovered_over: false,
        is_active: false,
        is_focused: false,
    }).collect::<Vec<_>>();

    for (_depth, parent_id) in node_depths_sorted {

        // Note: :nth-child() starts at 1 instead of 0
        let index_in_parent = parent_id.preceding_siblings(node_hierarchy).count();

        let is_parent_hovered_over = hovered_items.contains_key(parent_id);
        let parent_html_matcher = HtmlCascadeInfo {
            node_data: &input[*parent_id],
            index_in_parent: index_in_parent, // necessary for nth-child
            is_last_child: node_hierarchy[*parent_id].next_sibling.is_none(), // Necessary for :last selectors
            is_hovered_over: is_parent_hovered_over,
            is_active: is_parent_hovered_over && is_mouse_down,
            is_focused: focused_item == Some(*parent_id),
        };

        nodes[parent_id.index()] = parent_html_matcher;

        for (child_idx, child_id) in parent_id.children(node_hierarchy).enumerate() {
            let is_child_hovered_over = hovered_items.contains_key(&child_id);
            let child_html_matcher = HtmlCascadeInfo {
                node_data: &input[child_id],
                index_in_parent: child_idx + 1, // necessary for nth-child
                is_last_child: node_hierarchy[child_id].next_sibling.is_none(),
                is_hovered_over: is_child_hovered_over,
                is_active: is_child_hovered_over && is_mouse_down,
                is_focused: focused_item == Some(child_id),
            };

            nodes[child_id.index()] = child_html_matcher;
        }
    }

    NodeDataContainer { internal: nodes }
}

/// In order to support :hover, the element must have a TagId, otherwise it
/// will be disregarded in the hit-testing. A hover group
#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub struct HoverGroup {
    /// Whether any property in the hover group will trigger a re-layout.
    /// This is important for creating
    pub affects_layout: bool,
    /// Whether this path ends with `:active` or with `:hover`
    pub active_or_hover: ActiveHover,
}

/// Sets whether an element needs to be selected for `:active` or for `:hover`
#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub enum ActiveHover {
    Active,
    Hover,
}

/// Returns all CSS paths that have a `:hover` or `:active` in their path
/// (since they need to have tags for hit-testing)
fn collect_hover_groups(css: &Css) -> BTreeMap<CssPath, HoverGroup> {
    use azul_css::{CssPathSelector::*, CssPathPseudoSelector::*};

    let hover_rule = PseudoSelector(Hover);
    let active_rule = PseudoSelector(Active);

    // Filter out all :hover and :active rules, since we need to create tags
    // for them after the main CSS styling has been done
    css.rules().filter_map(|rule_block| {
        let pos = rule_block.path.selectors.iter().position(|x| *x == hover_rule || *x == active_rule)?;
        if rule_block.declarations.is_empty() {
            return None;
        }

        let active_or_hover = match rule_block.path.selectors.get(pos)? {
            PseudoSelector(Hover) => ActiveHover::Hover,
            PseudoSelector(Active) => ActiveHover::Active,
            _ => return None,
        };

        let css_path = CssPath { selectors: rule_block.path.selectors.iter().cloned().take(pos).collect() };
        let hover_group = HoverGroup {
            affects_layout: rule_block.declarations.iter().any(|hover_rule| hover_rule.can_trigger_relayout()),
            active_or_hover,
        };
        Some((css_path, hover_group))
    }).collect()
}

/// In order to figure out on which nodes to insert the :hover and :active hit-test tags,
/// we need to select all items that have a :hover or :active tag.
fn match_hover_selectors<'a, T: Layout>(
    hover_selectors: BTreeMap<CssPath, HoverGroup>,
    node_hierarchy: &NodeHierarchy,
    html_node_tree: &NodeDataContainer<HtmlCascadeInfo<'a, T>>,
) -> BTreeMap<NodeId, HoverGroup>
{
    let mut btree_map = BTreeMap::new();

    for (css_path, hover_selector) in hover_selectors {
        btree_map.extend(
            html_node_tree
            .linear_iter()
            .filter(|node_id| matches_html_element(&css_path, *node_id, node_hierarchy, html_node_tree))
            .map(|node_id| (node_id, hover_selector))
        );
    }

    btree_map
}

/// Matches a single group of items, panics on Children or DirectChildren selectors
///
/// The intent is to "split" the CSS path into groups by selectors, then store and cache
/// whether the direct or any parent has matched the path correctly
fn selector_group_matches<'a, T: Layout>(selectors: &[&CssPathSelector], html_node: &HtmlCascadeInfo<'a, T>) -> bool {
    use self::CssPathSelector::*;

    for selector in selectors {
        match selector {
            Global => { },
            Type(t) => {
                if html_node.node_data.node_type.get_path() != *t {
                    return false;
                }
            },
            Class(c) => {
                if !html_node.node_data.classes.iter().any(|class| class.equals_str(c)) {
                    return false;
                }
            },
            Id(id) => {
                if !html_node.node_data.ids.iter().any(|html_id| html_id.equals_str(id)) {
                    return false;
                }
            },
            PseudoSelector(CssPathPseudoSelector::First) => {
                // Notice: index_in_parent is 1-indexed
                if html_node.index_in_parent != 1 { return false; }
            },
            PseudoSelector(CssPathPseudoSelector::Last) => {
                // Notice: index_in_parent is 1-indexed
                if !html_node.is_last_child { return false; }
            },
            PseudoSelector(CssPathPseudoSelector::NthChild(x)) => {
                match *x {
                    Number(value) => if html_node.index_in_parent != value { return false; },
                    Even => if html_node.index_in_parent % 2 == 0 { return false; },
                    Odd => if html_node.index_in_parent % 2 == 1 { return false; },
                    Pattern { repeat, offset } => if html_node.index_in_parent >= offset &&
                        ((html_node.index_in_parent - offset) % repeat != 0) { return false; },
                }
            },
            PseudoSelector(CssPathPseudoSelector::Hover) => {
                if !html_node.is_hovered_over { return false; }
            },
            PseudoSelector(CssPathPseudoSelector::Active) => {
                if !html_node.is_active { return false; }
            },
            PseudoSelector(CssPathPseudoSelector::Focus) => {
                if !html_node.is_focused { return false; }
            },
            DirectChildren | Children => {
                panic!("Unreachable: DirectChildren or Children in CSS path!");
            },
        }
    }

    true
}

pub(crate) fn match_dom_selectors<T: Layout>(
    ui_state: &UiState<T>,
    css: &Css,
    focused_node: &mut Option<NodeId>,
    pending_focus_target: &mut Option<FocusTarget>,
    hovered_nodes: &BTreeMap<NodeId, HitTestItem>,
    is_mouse_down: bool,
) -> UiDescription<T>
{
    use std::collections::BTreeMap;

    let root = ui_state.dom.root;
    let non_leaf_nodes = ui_state.dom.arena.node_layout.get_parents_sorted_by_depth();

    let mut styled_nodes = BTreeMap::<NodeId, StyledNode>::new();

    let mut html_tree = construct_html_cascade_tree(
        &ui_state.dom.arena.node_data,
        &ui_state.dom.arena.node_layout,
        &non_leaf_nodes,
        *focused_node,
        hovered_nodes,
        is_mouse_down,
    );

    // Update the current focused field if the callbacks of the
    // previous frame has overridden the focus field
    update_focus_from_callbacks(
        pending_focus_target,
        focused_node,
        &ui_state.dom.arena.node_layout,
        &mut html_tree,
    );

    for (_depth, parent_id) in non_leaf_nodes {

        let mut parent_rules = styled_nodes.get(&parent_id).cloned().unwrap_or_default();

        // Iterate through all CSS rules, test if they match
        // This is technically O(n ^ 2), however, there are usually not that many CSS blocks,
        // so the cost of this should be insignificant.
        for applying_rule in css.rules().filter(|rule| {
            matches_html_element(&rule.path, parent_id, &ui_state.dom.arena.node_layout, &html_tree)
        }) {
            parent_rules.css_constraints.extend(applying_rule.declarations.clone());
        }

        let inheritable_rules: Vec<CssDeclaration> = parent_rules.css_constraints.iter().filter(|prop| prop.is_inheritable()).cloned().collect();

        // For children: inherit from parents - filter children that themselves are not parents!
        for child_id in parent_id.children(&ui_state.dom.arena.node_layout) {
            let child_node = &ui_state.dom.arena.node_layout[child_id];
            match child_node.first_child {
                None => {

                    // Style children that themselves aren't parents
                    let mut child_rules = inheritable_rules.clone();

                    // Iterate through all style rules, test if they match
                    // This is technically O(n ^ 2), however, there are usually not that many style blocks,
                    // so the cost of this should be insignificant.
                    for applying_rule in css.rules().filter(|rule| {
                        matches_html_element(&rule.path, child_id, &ui_state.dom.arena.node_layout, &html_tree)
                    }) {
                        child_rules.extend(applying_rule.declarations.clone());
                    }

                    styled_nodes.insert(child_id, StyledNode { css_constraints: child_rules });
                },
                Some(_) => {
                    // For all children that themselves are parents, simply copy the inheritable rules
                    styled_nodes.insert(child_id, StyledNode { css_constraints: inheritable_rules.clone() });
                },
            }
        }

        styled_nodes.insert(parent_id, parent_rules);
    }

    // In order to hit-test :hover and :active nodes, need to select them first
    // (to insert their TagId later)
    let selected_hover_nodes = match_hover_selectors(
        collect_hover_groups(css),
        &ui_state.dom.arena.node_layout,
        &html_tree
    );

    UiDescription {

        // NOTE: this clone is necessary, otherwise we wouldn't be able to
        // update the UiState
        //
        // WARNING: The UIState can modify the `arena` with its copy of the Rc !
        // Be careful about appending things to the arena, since that could modify
        // the UiDescription without you knowing!
        //
        // NOTE: This deep-clones the entire arena, which may be a
        // performance-sensitive operation!

        ui_descr_arena: ui_state.dom.arena.clone(),
        ui_descr_root: root,
        styled_nodes: styled_nodes,
        default_style_of_node: StyledNode::default(),
        dynamic_css_overrides: ui_state.dynamic_css_overrides.clone(),
        selected_hover_nodes,
    }
}

/// Update the WindowStates focus node in case the previous
/// frames callbacks set the focus to a specific node
///
/// Takes the `WindowState.pending_focus_target` and `WindowState.focused_node`
/// and updates the `WindowState.focused_node` accordingly.
/// Should be called before ``
fn update_focus_from_callbacks<'a, T: 'a + Layout>(
    pending_focus_target: &mut Option<FocusTarget>,
    focused_node: &mut Option<NodeId>,
    node_hierarchy: &NodeHierarchy,
    html_node_tree: &mut NodeDataContainer<HtmlCascadeInfo<'a, T>>,
) {
    // `pending_focus_target` is `None` in most cases, since usually the callbacks
    // don't mess with the current focused item.
    let new_focus_target = match pending_focus_target {
        Some(s) => s.clone(),
        None => return,
    };

    match new_focus_target {
        FocusTarget::Id(node_id) => {
            if html_node_tree.len() < node_id.index() {
                *focused_node = Some(node_id);
            } else {
                warn!("Focusing on node with invalid ID: {}", node_id);
            }
        },
        FocusTarget::NoFocus => { *focused_node = None; },
        FocusTarget::Path(css_path) => {
            if let Some(new_focused_node_id) = html_node_tree.linear_iter()
            .find(|node_id| matches_html_element(&css_path, *node_id, &node_hierarchy, &html_node_tree)) {
                 *focused_node = Some(new_focused_node_id);
            } else {
                warn!("Could not find focus node for path: {}", css_path);
            }
        },
    }

    // Set all items to None, no matter what - this takes care of clearing the current
    // focused item, in case the `pending_focus_target` is set to `Some(FocusTarget::NoFocus)`.
    for html_node in &mut html_node_tree.internal {
        html_node.is_focused = false;
    }

    if let Some(focused_node) = focused_node {
        html_node_tree[*focused_node].is_focused = true;
    }

    *pending_focus_target = None;
}