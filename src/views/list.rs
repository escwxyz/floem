use std::{
    hash::{BuildHasherDefault, Hash},
    marker::PhantomData,
};

use leptos_reactive::{create_effect, ScopeDisposer};
use rustc_hash::FxHasher;
use smallvec::SmallVec;

use crate::{
    app::AppContext,
    context::{AppState, EventCx, UpdateCx},
    id::Id,
    view::{ChangeFlags, View},
};

pub(crate) type FxIndexSet<T> = indexmap::IndexSet<T, BuildHasherDefault<FxHasher>>;

#[derive(educe::Educe)]
#[educe(Debug)]
pub(crate) struct HashRun<T>(#[educe(Debug(ignore))] pub(crate) T);

pub struct List<V, VF, T>
where
    V: View,
    VF: Fn(AppContext, T) -> V + 'static,
    T: 'static,
{
    id: Id,
    children: Vec<Option<(V, ScopeDisposer)>>,
    view_fn: VF,
    phatom: PhantomData<T>,
    cx: AppContext,
}

pub fn list<IF, I, T, KF, K, VF, V>(
    cx: AppContext,
    each_fn: IF,
    key_fn: KF,
    view_fn: VF,
) -> List<V, VF, T>
where
    IF: Fn() -> I + 'static,
    I: IntoIterator<Item = T>,
    KF: Fn(&T) -> K + 'static,
    K: Eq + Hash + 'static,
    VF: Fn(AppContext, T) -> V + 'static,
    V: View + 'static,
    T: 'static,
{
    let id = cx.new_id();

    let mut child_cx = cx;
    child_cx.id = id;
    create_effect(cx.scope, move |prev_hash_run| {
        let items = each_fn();
        let items = items.into_iter().collect::<SmallVec<[_; 128]>>();
        let hashed_items = items.iter().map(&key_fn).collect::<FxIndexSet<_>>();
        let diff = if let Some(HashRun(prev_hash_run)) = prev_hash_run {
            let mut cmds = diff(&prev_hash_run, &hashed_items);
            let mut items = items
                .into_iter()
                .map(|i| Some(i))
                .collect::<SmallVec<[Option<_>; 128]>>();
            for added in &mut cmds.added {
                added.view = Some(items[added.at].take().unwrap());
            }
            cmds
        } else {
            let mut diff = Diff::default();
            for (i, item) in each_fn().into_iter().enumerate() {
                diff.added.push(DiffOpAdd {
                    at: i,
                    view: Some(item),
                });
            }
            diff
        };
        AppContext::update_state(id, diff, false);
        HashRun(hashed_items)
    });
    List {
        id,
        children: Vec::new(),
        view_fn,
        phatom: PhantomData::default(),
        cx: child_cx,
    }
}

impl<V: View + 'static, VF, T> View for List<V, VF, T>
where
    VF: Fn(AppContext, T) -> V + 'static,
{
    fn id(&self) -> Id {
        self.id
    }

    fn child(&mut self, id: Id) -> Option<&mut dyn View> {
        let child = self
            .children
            .iter_mut()
            .find(|v| v.as_ref().map(|(v, _)| v.id() == id).unwrap_or(false));
        if let Some(child) = child {
            child.as_mut().map(|(view, _)| view as &mut dyn View)
        } else {
            None
        }
    }

    fn update(
        &mut self,
        cx: &mut UpdateCx,
        state: Box<dyn std::any::Any>,
    ) -> crate::view::ChangeFlags {
        if let Ok(diff) = state.downcast() {
            apply_diff(
                self.cx,
                cx.app_state,
                *diff,
                &mut self.children,
                &self.view_fn,
            );
            cx.request_layout(self.id());
            cx.reset_children_layout(self.id);
            ChangeFlags::LAYOUT
        } else {
            ChangeFlags::empty()
        }
    }

    fn layout(&mut self, cx: &mut crate::context::LayoutCx) -> taffy::prelude::Node {
        cx.layout_node(self.id, true, |cx| {
            let nodes = self
                .children
                .iter_mut()
                .filter_map(|child| Some(child.as_mut()?.0.layout_main(cx)))
                .collect::<Vec<_>>();
            nodes
        })
    }

    fn compute_layout(&mut self, cx: &mut crate::context::LayoutCx) {
        for child in &mut self.children {
            if let Some((child, _)) = child.as_mut() {
                child.compute_layout_main(cx);
            }
        }
    }

    fn event(
        &mut self,
        cx: &mut EventCx,
        id_path: Option<&[Id]>,
        event: crate::event::Event,
    ) -> bool {
        for child in self.children.iter_mut() {
            if let Some((child, _)) = child.as_mut() {
                let id = child.id();
                if cx.should_send(id, &event) && child.event_main(cx, id_path, event.clone()) {
                    return true;
                }
            }
        }
        false
    }

    fn paint(&mut self, cx: &mut crate::context::PaintCx) {
        for child in self.children.iter_mut() {
            if let Some((child, _)) = child.as_mut() {
                child.paint_main(cx);
            }
        }
    }
}

#[derive(Debug)]
pub struct Diff<V> {
    pub(crate) removed: SmallVec<[DiffOpRemove; 8]>,
    pub(crate) moved: SmallVec<[DiffOpMove; 8]>,
    pub(crate) added: SmallVec<[DiffOpAdd<V>; 8]>,
    pub(crate) clear: bool,
}

impl<V> Default for Diff<V> {
    fn default() -> Self {
        Self {
            removed: Default::default(),
            moved: Default::default(),
            added: Default::default(),
            clear: false,
        }
    }
}

#[derive(Debug)]
pub(crate) struct DiffOpMove {
    from: usize,
    to: usize,
}

#[derive(Debug)]
pub(crate) struct DiffOpAdd<V> {
    pub(crate) at: usize,
    pub(crate) view: Option<V>,
}

#[derive(Debug)]
pub(crate) struct DiffOpRemove {
    at: usize,
}

/// Calculates the operations need to get from `a` to `b`.
pub(crate) fn diff<K: Eq + Hash, V>(from: &FxIndexSet<K>, to: &FxIndexSet<K>) -> Diff<V> {
    if from.is_empty() && to.is_empty() {
        return Diff::default();
    } else if to.is_empty() {
        return Diff {
            clear: true,
            ..Default::default()
        };
    }

    // Get removed items
    let mut removed = from.difference(to);

    let removed_cmds = removed
        .clone()
        .map(|k| from.get_full(k).unwrap().0)
        .map(|idx| DiffOpRemove { at: idx });

    // Get added items
    let mut added = to.difference(from);

    let added_cmds = added
        .clone()
        .map(|k| to.get_full(k).unwrap().0)
        .map(|idx| DiffOpAdd {
            at: idx,
            view: None,
        });

    // Get moved items
    let mut normalized_idx = 0;
    let mut move_cmds = SmallVec::<[_; 8]>::with_capacity(to.len());
    let mut added_idx = added.next().map(|k| to.get_full(k).unwrap().0);
    let mut removed_idx = removed.next().map(|k| from.get_full(k).unwrap().0);

    for (idx, k) in to.iter().enumerate() {
        if let Some(added_idx) = added_idx.as_mut().filter(|r_i| **r_i == idx) {
            if let Some(next_added) = added.next().map(|k| to.get_full(k).unwrap().0) {
                *added_idx = next_added;

                normalized_idx = usize::wrapping_sub(normalized_idx, 1);
            }
        }

        if let Some(removed_idx) = removed_idx.as_mut().filter(|r_i| **r_i == idx) {
            normalized_idx = normalized_idx.wrapping_add(1);

            if let Some(next_removed) = removed.next().map(|k| from.get_full(k).unwrap().0) {
                *removed_idx = next_removed;
            }
        }

        if let Some((from_idx, _)) = from.get_full(k) {
            if from_idx != normalized_idx || from_idx != idx {
                move_cmds.push(DiffOpMove {
                    from: from_idx,
                    to: idx,
                });
            }
        }

        normalized_idx = normalized_idx.wrapping_add(1);
    }

    let mut diffs = Diff {
        removed: removed_cmds.collect(),
        moved: move_cmds,
        added: added_cmds.collect(),
        clear: false,
    };

    if !from.is_empty()
        && !to.is_empty()
        && diffs.removed.len() == from.len()
        && diffs.moved.is_empty()
    {
        diffs.clear = true;
    }

    diffs
}

fn remove_index<V: View>(
    app_state: &mut AppState,
    children: &mut [Option<(V, ScopeDisposer)>],
    index: usize,
) -> Option<()> {
    let (view, disposer) = std::mem::take(&mut children[index])?;
    disposer.dispose();
    let id = view.id();
    if let Some(view_state) = app_state.view_states.remove(&id) {
        let node = view_state.node;
        let mut nodes = Vec::new();
        let mut parents = Vec::new();
        parents.push(node);
        nodes.push(node);
        while !parents.is_empty() {
            let parent = parents.pop().unwrap();
            if let Ok(children) = app_state.taffy.children(parent) {
                for child in children {
                    nodes.push(child);
                    parents.push(child);
                }
            }
        }
        for node in nodes {
            let _ = app_state.taffy.remove(node);
        }
    }

    let mut all_ids = id.all_chilren();
    all_ids.push(id);
    for id in all_ids {
        id.remove_idpath();
        app_state.view_states.remove(&id);
    }

    Some(())
}

pub(super) fn apply_diff<T, V, VF>(
    cx: AppContext,
    app_state: &mut AppState,
    mut diff: Diff<T>,
    children: &mut Vec<Option<(V, ScopeDisposer)>>,
    view_fn: &VF,
) where
    V: View,
    VF: Fn(AppContext, T) -> V + 'static,
{
    // Resize children if needed
    if diff.added.len().checked_sub(diff.removed.len()).is_some() {
        let target_size =
            children.len() + (diff.added.len() as isize - diff.removed.len() as isize) as usize;

        children.resize_with(target_size, || None);
    }

    // We need to hold a list of items which will be moved, and
    // we can only perform the move after all commands have run, otherwise,
    // we risk overwriting one of the values
    let mut items_to_move = Vec::with_capacity(diff.moved.len());

    // The order of cmds needs to be:
    // 1. Clear
    // 2. Removed
    // 3. Moved
    // 4. Add
    if diff.clear {
        for i in 0..children.len() {
            remove_index(app_state, children, i);
        }
        diff.removed.clear();
    }

    for DiffOpRemove { at } in diff.removed {
        remove_index(app_state, children, at);
    }

    for DiffOpMove { from, to } in diff.moved {
        let item = std::mem::take(&mut children[from]).unwrap();
        items_to_move.push((to, item));
    }

    for DiffOpAdd { at, view } in diff.added {
        children[at] = view.map(|value| {
            cx.scope.run_child_scope(|scope| {
                let mut cx = cx;
                cx.scope = scope;
                view_fn(cx, value)
            })
        });
    }

    for (to, each_item) in items_to_move {
        children[to] = Some(each_item);
    }

    // Now, remove the holes that might have been left from removing
    // items
    children.retain(|c| c.is_some());
}
