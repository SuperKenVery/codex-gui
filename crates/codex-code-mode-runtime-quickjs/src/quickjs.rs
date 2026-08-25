use std::sync::Arc;
use std::time::Duration;

use codex_code_mode_protocol::FunctionCallOutputContentItem;
use codex_code_mode_protocol::ImageDetail;
use codex_code_mode_protocol::ToolDefinition;
use rquickjs::AsyncContext;
use rquickjs::AsyncRuntime;
use rquickjs::CatchResultExt;
use rquickjs::CaughtError;
use rquickjs::Module;
use rquickjs::prelude::Async;
use rquickjs::prelude::Func;
use rquickjs::prelude::Opt;
use serde_json::Value as JsonValue;

use crate::bridge::RuntimeBridge;

const EXIT_SENTINEL: &str = "__codex_code_mode_exit__";
const HELPER_SOURCE: &str = include_str!("helpers.js");

pub(crate) async fn execute_javascript(
    source: String,
    bridge: Arc<RuntimeBridge>,
    memory_limit: Option<usize>,
) -> Result<(), String> {
    let runtime = AsyncRuntime::new().map_err(|error| error.to_string())?;
    if let Some(memory_limit) = memory_limit {
        runtime.set_memory_limit(memory_limit).await;
    }
    let cancellation = bridge.cell().cancellation.clone();
    runtime
        .set_interrupt_handler(Some(Box::new(move || cancellation.is_cancelled())))
        .await;
    let context = AsyncContext::full(&runtime)
        .await
        .map_err(|error| error.to_string())?;

    let result = context
        .async_with(async |ctx| {
            install_native_functions(&ctx, Arc::clone(&bridge))
                .catch(&ctx)
                .map_err(format_caught_error)?;
            let metadata = tool_metadata_json(bridge.enabled_tools());
            let metadata = ctx
                .json_parse(metadata)
                .catch(&ctx)
                .map_err(format_caught_error)?;
            ctx.globals()
                .set("__codex_tool_metadata", metadata)
                .catch(&ctx)
                .map_err(format_caught_error)?;
            ctx.eval::<(), _>(HELPER_SOURCE)
                .catch(&ctx)
                .map_err(format_caught_error)?;

            let promise = Module::evaluate(ctx.clone(), "exec_main.mjs", source)
                .catch(&ctx)
                .map_err(format_caught_error)?;
            promise
                .into_future::<()>()
                .await
                .catch(&ctx)
                .map_err(format_caught_error)
        })
        .await;

    match result {
        Ok(()) => Ok(()),
        Err(error) if bridge.exit_requested() && error.contains(EXIT_SENTINEL) => Ok(()),
        Err(error) => Err(error),
    }
}

fn install_native_functions<'js>(
    ctx: &rquickjs::Ctx<'js>,
    bridge: Arc<RuntimeBridge>,
) -> rquickjs::Result<()> {
    let globals = ctx.globals();

    let emit_bridge = Arc::clone(&bridge);
    globals.set(
        "__codex_emit_text",
        Func::from(move |text: String| {
            emit_bridge
                .cell()
                .push(FunctionCallOutputContentItem::InputText { text });
        }),
    )?;

    let image_bridge = Arc::clone(&bridge);
    globals.set(
        "__codex_emit_image",
        Func::from(move |image_url: String, detail: Opt<String>| {
            let detail = detail.0.and_then(|detail| match detail.as_str() {
                "auto" => Some(ImageDetail::Auto),
                "low" => Some(ImageDetail::Low),
                "high" => Some(ImageDetail::High),
                "original" => Some(ImageDetail::Original),
                _ => None,
            });
            image_bridge
                .cell()
                .push(FunctionCallOutputContentItem::InputImage { image_url, detail });
        }),
    )?;

    let audio_bridge = Arc::clone(&bridge);
    globals.set(
        "__codex_emit_audio",
        Func::from(move |audio_url: String| {
            audio_bridge
                .cell()
                .push(FunctionCallOutputContentItem::InputAudio { audio_url });
        }),
    )?;

    let tool_bridge = Arc::clone(&bridge);
    globals.set(
        "__codex_invoke_tool",
        Func::from(Async(move |index: i32, input: Opt<String>| {
            let tool_bridge = Arc::clone(&tool_bridge);
            async move {
                Ok::<String, rquickjs::Error>(tool_bridge.invoke_tool(index, input.0).await)
            }
        })),
    )?;

    let notify_bridge = Arc::clone(&bridge);
    globals.set(
        "__codex_notify",
        Func::from(move |text: String| notify_bridge.notify(text)),
    )?;

    let yield_bridge = Arc::clone(&bridge);
    globals.set(
        "__codex_yield",
        Func::from(move || yield_bridge.cell().request_yield()),
    )?;

    let store_bridge = Arc::clone(&bridge);
    globals.set(
        "__codex_store",
        Func::from(move |key: String, json: String| {
            store_bridge.store(key, json).unwrap_or_else(|error| {
                tracing::warn!(%error, "QuickJS code-mode store failed");
            });
        }),
    )?;

    let load_bridge = Arc::clone(&bridge);
    globals.set(
        "__codex_load",
        Func::from(move |key: String| load_bridge.load(&key)),
    )?;

    let exit_bridge = Arc::clone(&bridge);
    globals.set(
        "__codex_exit",
        Func::from(move || exit_bridge.request_exit()),
    )?;

    globals.set("__codex_sleep", Func::from(Async(sleep_ms)))?;
    Ok(())
}

async fn sleep_ms(delay_ms: f64) -> rquickjs::Result<()> {
    let delay_ms = if delay_ms.is_finite() && delay_ms > 0.0 {
        delay_ms.trunc().min(u64::MAX as f64) as u64
    } else {
        0
    };
    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    Ok(())
}

fn tool_metadata_json(tools: &[ToolDefinition]) -> String {
    JsonValue::Array(
        tools
            .iter()
            .enumerate()
            .map(|(index, tool)| {
                serde_json::json!({
                    "index": index,
                    "name": tool.name,
                    "description": tool.description,
                })
            })
            .collect(),
    )
    .to_string()
}

fn format_caught_error(error: CaughtError<'_>) -> String {
    match error {
        CaughtError::Exception(exception) => exception
            .stack()
            .or_else(|| exception.message())
            .unwrap_or_else(|| "unknown QuickJS exception".to_string()),
        other => other.to_string(),
    }
}
