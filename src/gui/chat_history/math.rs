use std::{
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex, OnceLock},
};

use gpui::{
    AnyElement, App, Hsla, Image, ImageFormat, IntoElement, ObjectFit, Rgba, Window, div, img,
    prelude::*, px, relative,
};
use gpui_component::{
    ActiveTheme as _,
    text::{MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast},
};
use ratex_layout::{LayoutOptions, layout, to_display_list};
use ratex_parser::parser::parse;
use ratex_svg::{SvgColorSyntax, SvgOptions, render_to_svg_with_color_syntax};
use ratex_types::{
    color::Color,
    display_item::{DisplayItem, DisplayList},
};

const MAX_FORMULA_BYTES: usize = 16 * 1024;

#[derive(Clone)]
pub(super) struct MathPlugin;

impl MathPlugin {
    pub(super) fn new() -> Self {
        Self
    }
}

#[derive(Clone)]
struct MathFormula {
    source: String,
    markdown: String,
    display_list: Option<Arc<DisplayList>>,
}

#[derive(Clone)]
struct RenderedMathImage {
    image: Arc<Image>,
    width: f32,
    height: f32,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ImageCacheKey {
    source: String,
    font_size_bits: u32,
    color: u32,
}

impl MarkdownPlugin for MathPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "ratex-math"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        if let markdown_ast::Node::Math(math) = node {
            let markdown = cx
                .node_source(node)
                .map(str::to_string)
                .unwrap_or_else(|| format!("$$\n{}\n$$", math.value));
            return Some(math_node(MathFormula::new(math.value.clone(), markdown)));
        }

        let markdown_ast::Node::Paragraph(_) = node else {
            return None;
        };
        let markdown = cx.node_source(node)?;
        let source = block_formula(markdown)?;
        Some(math_node(MathFormula::new(
            source.to_string(),
            markdown.trim().to_string(),
        )))
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let formula = node
            .data::<MathFormula>()
            .expect("RaTeX markdown node must contain MathFormula data");
        let font_size = f32::from(window.text_style().font_size.to_pixels(window.rem_size()));

        div()
            .w_full()
            .flex()
            .justify_center()
            .py_1()
            .child(render_math_formula(formula, font_size, cx))
    }
}

impl MathFormula {
    fn new(source: String, markdown: String) -> Self {
        let display_list = render_display_list(&source).map(Arc::new);
        Self {
            source,
            markdown,
            display_list,
        }
    }
}

fn math_node(formula: MathFormula) -> MarkdownNode {
    MarkdownNode::new("ratex-math", formula.clone())
        .text(formula.markdown.clone())
        .markdown(formula.markdown)
}

fn block_formula(source: &str) -> Option<&str> {
    let source = source.trim();
    let body = if let Some(body) = source
        .strip_prefix("$$")
        .and_then(|body| body.strip_suffix("$$"))
    {
        body
    } else {
        source.strip_prefix(r"\[")?.strip_suffix(r"\]")?
    }
    .trim();

    (!body.is_empty()).then_some(body)
}

fn render_display_list(source: &str) -> Option<DisplayList> {
    if source.is_empty() || source.len() > MAX_FORMULA_BYTES {
        return None;
    }

    catch_unwind(AssertUnwindSafe(|| {
        let ast = parse(source).ok()?;
        Some(to_display_list(&layout(&ast, &LayoutOptions::default())))
    }))
    .ok()
    .flatten()
}

fn render_math_formula(formula: &MathFormula, base_font_size: f32, cx: &mut App) -> AnyElement {
    let foreground = cx.theme().foreground;
    if let Some(image) = render_math_image(formula, base_font_size, foreground) {
        img(image.image)
            .object_fit(ObjectFit::Contain)
            .flex_shrink_0()
            .max_w(relative(1.))
            .w(px(image.width))
            .h(px(image.height))
            .into_any_element()
    } else {
        div()
            .flex_none()
            .line_height(relative(1.2))
            .text_size(px(base_font_size * 1.18))
            .text_color(foreground)
            .italic()
            .child(formula.markdown.clone())
            .into_any_element()
    }
}

fn render_math_image(
    formula: &MathFormula,
    base_font_size: f32,
    foreground: Hsla,
) -> Option<RenderedMathImage> {
    static CACHE: OnceLock<Mutex<HashMap<ImageCacheKey, RenderedMathImage>>> = OnceLock::new();

    let display_list = formula.display_list.as_ref()?;
    let font_size = (base_font_size * 1.18).max(12.0);
    let rgba = Rgba::from(foreground);
    let key = ImageCacheKey {
        source: formula.source.clone(),
        font_size_bits: font_size.to_bits(),
        color: u32::from(rgba),
    };
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(cache) = cache.lock()
        && let Some(image) = cache.get(&key)
    {
        return Some(image.clone());
    }

    let padding = 2.0;
    let mut colored = display_list.as_ref().clone();
    recolor_display_list(&mut colored, Color::new(rgba.r, rgba.g, rgba.b, rgba.a));
    let options = SvgOptions {
        font_size: font_size.into(),
        padding,
        stroke_width: f64::from(font_size) * 0.0375,
        embed_glyphs: true,
        font_dir: String::new(),
    };
    let svg = render_to_svg_with_color_syntax(&colored, &options, SvgColorSyntax::Rgb);
    let width = (colored.width * f64::from(font_size) + 2.0 * padding) as f32;
    let height = (colored.total_height() * f64::from(font_size) + 2.0 * padding) as f32;
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return None;
    }

    let image = RenderedMathImage {
        width,
        height,
        image: Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes())),
    };
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, image.clone());
    }
    Some(image)
}

fn recolor_display_list(display_list: &mut DisplayList, color: Color) {
    for item in &mut display_list.items {
        match item {
            DisplayItem::GlyphPath {
                color: item_color, ..
            }
            | DisplayItem::Line {
                color: item_color, ..
            }
            | DisplayItem::Rect {
                color: item_color, ..
            }
            | DisplayItem::Path {
                color: item_color, ..
            } => *item_color = color,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_both_display_delimiters() {
        assert_eq!(block_formula("$$\nx^2\n$$"), Some("x^2"));
        assert_eq!(block_formula(r"\[\frac{1}{2}\]"), Some(r"\frac{1}{2}"));
    }

    #[test]
    fn does_not_claim_inline_or_mixed_paragraphs() {
        assert!(block_formula(r"Euler: $e^{i\pi}+1=0$.").is_none());
        assert!(block_formula("before $$x^2$$ after").is_none());
        assert!(block_formula(r"energy: \(E=mc^2\).").is_none());
    }

    #[test]
    fn incomplete_formula_remains_plain_markdown() {
        assert!(block_formula("$$\nx +").is_none());
        assert!(block_formula(r"\[x +").is_none());
    }

    #[test]
    fn ratex_builds_a_non_empty_display_list() {
        let list = render_display_list(r"\frac{-b \pm \sqrt{b^2-4ac}}{2a}")
            .expect("valid LaTeX should render");
        assert!(list.width > 0.0);
        assert!(list.total_height() > 0.0);
        assert!(!list.items.is_empty());
    }
}
