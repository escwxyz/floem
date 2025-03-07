use std::{hash::Hash, marker::PhantomData, ops::Range};

use glazier::kurbo::Rect;
use leptos_reactive::{
    create_effect, create_signal, ScopeDisposer, SignalGet, SignalSet, WriteSignal,
};
use smallvec::SmallVec;
use taffy::{prelude::Node, style::Dimension};

use crate::{
    app::AppContext,
    context::LayoutCx,
    id::Id,
    view::{ChangeFlags, View},
};

use super::{apply_diff, diff, Diff, DiffOpAdd, FxIndexSet, HashRun};

#[derive(Clone, Copy)]
pub enum VirtualListDirection {
    Vertical,
    Horizontal,
}

pub enum VirtualListItemSize<T> {
    Fn(Box<dyn Fn(&T) -> f64>),
    Fixed(f64),
}

pub trait VirtualListVector<T> {
    type ItemIterator: Iterator<Item = T>;

    fn total_len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.total_len() == 0
    }

    fn slice(&mut self, range: Range<usize>) -> Self::ItemIterator;
}

pub struct VirtualList<V: View, VF, T>
where
    VF: Fn(AppContext, T) -> V + 'static,
    T: 'static,
{
    id: Id,
    direction: VirtualListDirection,
    children: Vec<Option<(V, ScopeDisposer)>>,
    viewport: Rect,
    set_viewport: WriteSignal<Rect>,
    view_fn: VF,
    phatom: PhantomData<T>,
    cx: AppContext,
    before_size: f64,
    after_size: f64,
    before_node: Option<Node>,
    after_node: Option<Node>,
}

struct VirtualListState<T> {
    diff: Diff<T>,
    before_size: f64,
    after_size: f64,
}

pub fn virtual_list<T, IF, I, KF, K, VF, V>(
    cx: AppContext,
    direction: VirtualListDirection,
    each_fn: IF,
    key_fn: KF,
    view_fn: VF,
    item_size: VirtualListItemSize<T>,
) -> VirtualList<V, VF, T>
where
    T: 'static,
    IF: Fn() -> I + 'static,
    I: VirtualListVector<T>,
    KF: Fn(&T) -> K + 'static,
    K: Eq + Hash + 'static,
    VF: Fn(AppContext, T) -> V + 'static,
    V: View + 'static,
{
    let id = cx.new_id();

    let mut child_cx = cx;
    child_cx.id = id;

    let (viewport, set_viewport) = create_signal(cx.scope, Rect::ZERO);

    create_effect(cx.scope, move |prev_hash_run| {
        let mut items_vector = each_fn();
        let viewport = viewport.get();
        let min = match direction {
            VirtualListDirection::Vertical => viewport.y0,
            VirtualListDirection::Horizontal => viewport.x0,
        };
        let max = match direction {
            VirtualListDirection::Vertical => viewport.height() + viewport.y0,
            VirtualListDirection::Horizontal => viewport.width() + viewport.x0,
        };
        let mut main_axis = 0.0;
        let mut items = Vec::new();

        let mut before_size = 0.0;
        let mut after_size = 0.0;
        match &item_size {
            VirtualListItemSize::Fixed(item_size) => {
                let item_size = *item_size;
                let total_len = items_vector.total_len();
                let start = if item_size > 0.0 {
                    (min / item_size).floor() as usize
                } else {
                    0
                };
                let end = if item_size > 0.0 {
                    ((max / item_size).ceil() as usize).min(total_len)
                } else {
                    usize::MAX
                };
                before_size = item_size * start as f64;

                for item in items_vector.slice(start..end) {
                    items.push(item);
                }

                after_size = item_size * (total_len.saturating_sub(end)) as f64;
            }
            VirtualListItemSize::Fn(size_fn) => {
                let total_len = items_vector.total_len();
                for item in items_vector.slice(0..total_len) {
                    let item_size = size_fn(&item);
                    if main_axis < min {
                        main_axis += item_size;
                        before_size += item_size;
                        continue;
                    }

                    if main_axis <= max {
                        items.push(item);
                    } else {
                        after_size += item_size;
                    }
                }
            }
        };

        let hashed_items = items.iter().map(&key_fn).collect::<FxIndexSet<_>>();
        let diff = if let Some(HashRun(prev_hash_run)) = prev_hash_run {
            let mut diff = diff(&prev_hash_run, &hashed_items);
            let mut items = items
                .into_iter()
                .map(|i| Some(i))
                .collect::<SmallVec<[Option<_>; 128]>>();
            for added in &mut diff.added {
                added.view = Some(items[added.at].take().unwrap());
            }
            diff
        } else {
            let mut diff = Diff::default();
            for (i, item) in items.into_iter().enumerate() {
                diff.added.push(DiffOpAdd {
                    at: i,
                    view: Some(item),
                });
            }
            diff
        };
        AppContext::update_state(
            id,
            VirtualListState {
                diff,
                before_size,
                after_size,
            },
            false,
        );
        HashRun(hashed_items)
    });

    VirtualList {
        id,
        direction,
        children: Vec::new(),
        viewport: Rect::ZERO,
        set_viewport,
        view_fn,
        phatom: PhantomData::default(),
        cx: child_cx,
        before_size: 0.0,
        after_size: 0.0,
        before_node: None,
        after_node: None,
    }
}

impl<V: View + 'static, VF, T> View for VirtualList<V, VF, T>
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
        cx: &mut crate::context::UpdateCx,
        state: Box<dyn std::any::Any>,
    ) -> crate::view::ChangeFlags {
        if let Ok(state) = state.downcast::<VirtualListState<T>>() {
            self.before_size = state.before_size;
            self.after_size = state.after_size;
            apply_diff(
                self.cx,
                cx.app_state,
                state.diff,
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
            let mut nodes = self
                .children
                .iter_mut()
                .filter_map(|child| Some(child.as_mut()?.0.layout_main(cx)))
                .collect::<Vec<_>>();
            let before_size = match self.direction {
                VirtualListDirection::Vertical => taffy::prelude::Size {
                    width: Dimension::Percent(1.0),
                    height: Dimension::Points(self.before_size as f32),
                },
                VirtualListDirection::Horizontal => taffy::prelude::Size {
                    width: Dimension::Points(self.before_size as f32),
                    height: Dimension::Percent(1.0),
                },
            };
            let after_size = match self.direction {
                VirtualListDirection::Vertical => taffy::prelude::Size {
                    width: Dimension::Percent(1.0),
                    height: Dimension::Points(self.after_size as f32),
                },
                VirtualListDirection::Horizontal => taffy::prelude::Size {
                    width: Dimension::Points(self.after_size as f32),
                    height: Dimension::Percent(1.0),
                },
            };
            if self.before_node.is_none() {
                self.before_node = Some(
                    cx.app_state
                        .taffy
                        .new_leaf(taffy::style::Style::DEFAULT)
                        .unwrap(),
                );
            }
            if self.after_node.is_none() {
                self.after_node = Some(
                    cx.app_state
                        .taffy
                        .new_leaf(taffy::style::Style::DEFAULT)
                        .unwrap(),
                );
            }
            let before_node = self.before_node.unwrap();
            let after_node = self.after_node.unwrap();
            let _ = cx.app_state.taffy.set_style(
                before_node,
                taffy::style::Style {
                    size: before_size,
                    ..Default::default()
                },
            );
            let _ = cx.app_state.taffy.set_style(
                after_node,
                taffy::style::Style {
                    size: after_size,
                    ..Default::default()
                },
            );
            nodes.insert(0, before_node);
            nodes.push(after_node);
            nodes
        })
    }

    fn compute_layout(&mut self, cx: &mut LayoutCx) {
        let viewport = cx.viewport.unwrap_or_default();
        if self.viewport != viewport {
            self.viewport = viewport;
            self.set_viewport.set(viewport);
        }

        for child in &mut self.children {
            if let Some((child, _)) = child.as_mut() {
                child.compute_layout_main(cx);
            }
        }
    }

    fn event(
        &mut self,
        cx: &mut crate::context::EventCx,
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
        for child in &mut self.children {
            if let Some((child, _)) = child.as_mut() {
                child.paint_main(cx);
            }
        }
    }
}

impl<T: Clone> VirtualListVector<T> for im::Vector<T> {
    type ItemIterator = im::vector::ConsumingIter<T>;

    fn total_len(&self) -> usize {
        self.len()
    }

    fn slice(&mut self, range: Range<usize>) -> Self::ItemIterator {
        self.slice(range).into_iter()
    }
}
