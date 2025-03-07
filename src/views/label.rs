use std::any::Any;

use crate::{
    cosmic_text::{Attrs, AttrsList, FamilyOwned, TextLayout},
    style::ReifiedStyle,
};
use floem_renderer::{
    cosmic_text::{Style as FontStyle, Weight},
    Renderer,
};
use glazier::kurbo::Point;
use leptos_reactive::create_effect;
use taffy::{prelude::Node, style::Dimension};
use vello::peniko::Color;

use crate::{
    app::AppContext,
    context::{EventCx, UpdateCx},
    event::Event,
    id::Id,
    style::Style,
    view::{ChangeFlags, View},
};

pub struct Label {
    id: Id,
    label: String,
    text_layout: Option<TextLayout>,
    text_node: Option<Node>,
    available_text: Option<String>,
    available_width: Option<f32>,
    available_text_layout: Option<TextLayout>,
    color: Option<Color>,
    font_size: Option<f32>,
    font_family: Option<String>,
    font_weight: Option<Weight>,
    font_style: Option<FontStyle>,
}

pub fn label(cx: AppContext, label: impl Fn() -> String + 'static) -> Label {
    let id = cx.new_id();
    create_effect(cx.scope, move |_| {
        let new_label = label();
        AppContext::update_state(id, new_label, false);
    });
    Label {
        id,
        label: "".to_string(),
        text_layout: None,
        text_node: None,
        available_text: None,
        available_width: None,
        available_text_layout: None,
        color: None,
        font_size: None,
        font_family: None,
        font_weight: None,
        font_style: None,
    }
}

impl Label {
    fn set_text_layout(&mut self) {
        let mut text_layout = TextLayout::new();
        let mut attrs = Attrs::new().color(self.color.unwrap_or(Color::BLACK));
        if let Some(font_size) = self.font_size {
            attrs = attrs.font_size(font_size);
        }
        if let Some(font_style) = self.font_style {
            attrs = attrs.style(font_style);
        }
        let font_family = self.font_family.as_ref().map(|font_family| {
            let family: Vec<FamilyOwned> = FamilyOwned::parse_list(font_family).collect();
            family
        });
        if let Some(font_family) = font_family.as_ref() {
            attrs = attrs.family(font_family);
        }
        if let Some(font_weight) = self.font_weight {
            attrs = attrs.weight(font_weight);
        }
        text_layout.set_text(self.label.as_str(), AttrsList::new(attrs));
        self.text_layout = Some(text_layout);

        if let Some(new_text) = self.available_text.as_ref() {
            let mut text_layout = TextLayout::new();
            let mut attrs = Attrs::new().color(self.color.unwrap_or(Color::BLACK));
            if let Some(font_size) = self.font_size {
                attrs = attrs.font_size(font_size);
            }
            if let Some(font_style) = self.font_style {
                attrs = attrs.style(font_style);
            }
            let font_family = self.font_family.as_ref().map(|font_family| {
                let family: Vec<FamilyOwned> = FamilyOwned::parse_list(font_family).collect();
                family
            });
            if let Some(font_family) = font_family.as_ref() {
                attrs = attrs.family(font_family);
            }
            if let Some(font_weight) = self.font_weight {
                attrs = attrs.weight(font_weight);
            }
            text_layout.set_text(new_text, AttrsList::new(attrs));
            self.available_text_layout = Some(text_layout);
        }
    }
}

impl View for Label {
    fn id(&self) -> Id {
        self.id
    }

    fn child(&mut self, _id: Id) -> Option<&mut dyn View> {
        None
    }

    fn update(&mut self, cx: &mut UpdateCx, state: Box<dyn Any>) -> ChangeFlags {
        if let Ok(state) = state.downcast() {
            self.label = *state;
            self.text_layout = None;
            cx.request_layout(self.id());
            ChangeFlags::LAYOUT
        } else {
            ChangeFlags::empty()
        }
    }

    fn event(&mut self, _cx: &mut EventCx, _id_path: Option<&[Id]>, _event: Event) -> bool {
        false
    }

    fn layout(&mut self, cx: &mut crate::context::LayoutCx) -> taffy::prelude::Node {
        cx.layout_node(self.id, true, |cx| {
            let (width, height) = if self.label.is_empty() {
                (0.0, cx.current_font_size().unwrap_or(12.0))
            } else {
                if self.font_size != cx.current_font_size()
                    || self.font_family.as_deref() != cx.current_font_family()
                    || self.font_weight != cx.font_weight
                    || self.font_style != cx.font_style
                {
                    self.font_size = cx.current_font_size();
                    self.font_family = cx.current_font_family().map(|s| s.to_string());
                    self.font_weight = cx.font_weight;
                    self.font_style = cx.font_style;
                    self.set_text_layout();
                }
                if self.text_layout.is_none() {
                    self.set_text_layout();
                }
                let text_layout = self.text_layout.as_ref().unwrap();
                let size = text_layout.size();
                let width = size.width.ceil() as f32;
                let height = size.height as f32;
                (width, height)
            };

            if self.text_node.is_none() {
                self.text_node = Some(
                    cx.app_state
                        .taffy
                        .new_leaf(taffy::style::Style::DEFAULT)
                        .unwrap(),
                );
            }
            let text_node = self.text_node.unwrap();

            let style = Style::default()
                .width(Dimension::Points(width))
                .height(Dimension::Points(height))
                .reify(&ReifiedStyle::default())
                .to_taffy_style();
            let _ = cx.app_state.taffy.set_style(text_node, style);

            vec![text_node]
        })
    }

    fn compute_layout(&mut self, cx: &mut crate::context::LayoutCx) {
        if self.label.is_empty() {
            return;
        }

        let text_node = self.text_node.unwrap();
        let layout = cx.app_state.taffy.layout(text_node).unwrap();
        let text_layout = self.text_layout.as_ref().unwrap();
        let width = text_layout.size().width as f32;
        if width > layout.size.width {
            if self.available_width != Some(layout.size.width) {
                let mut dots_text = TextLayout::new();
                let mut attrs = Attrs::new().color(self.color.unwrap_or(Color::BLACK));
                if let Some(font_size) = self.font_size {
                    attrs = attrs.font_size(font_size);
                }
                if let Some(font_style) = self.font_style {
                    attrs = attrs.style(font_style);
                }
                let font_family = self.font_family.as_ref().map(|font_family| {
                    let family: Vec<FamilyOwned> = FamilyOwned::parse_list(font_family).collect();
                    family
                });
                if let Some(font_family) = font_family.as_ref() {
                    attrs = attrs.family(font_family);
                }
                if let Some(font_weight) = self.font_weight {
                    attrs = attrs.weight(font_weight);
                }
                dots_text.set_text("...", AttrsList::new(attrs));

                let dots_width = dots_text.size().width as f32;
                let width_left = layout.size.width - dots_width;
                let hit_point = text_layout.hit_point(Point::new(width_left as f64, 0.0));
                let index = hit_point.index;

                let new_text = if index > 0 {
                    format!("{}...", &self.label[..index])
                } else {
                    "".to_string()
                };
                self.available_text = Some(new_text);
                self.available_width = Some(layout.size.width);
                self.set_text_layout();
            }
        } else {
            self.available_text = None;
            self.available_width = None;
            self.available_text_layout = None;
        }
    }

    fn paint(&mut self, cx: &mut crate::context::PaintCx) {
        if self.label.is_empty() {
            return;
        }

        if self.color != cx.color
            || self.font_size != cx.font_size
            || self.font_family.as_deref() != cx.font_family.as_deref()
            || self.font_weight != cx.font_weight
            || self.font_style != cx.font_style
        {
            self.color = cx.color;
            self.font_size = cx.font_size;
            self.font_family = cx.font_family.clone();
            self.font_weight = cx.font_weight;
            self.font_style = cx.font_style;
            self.set_text_layout();
        }
        let text_node = self.text_node.unwrap();
        let location = cx.app_state.taffy.layout(text_node).unwrap().location;
        let point = Point::new(location.x as f64, location.y as f64);
        if let Some(text_layout) = self.available_text_layout.as_ref() {
            cx.draw_text(text_layout, point);
        } else {
            cx.draw_text(self.text_layout.as_ref().unwrap(), point);
        }
    }
}
