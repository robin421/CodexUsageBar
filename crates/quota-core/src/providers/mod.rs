pub mod claude;
pub mod codebuff;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod gemini;
pub mod jetbrains;
pub mod keyed;
pub mod opencode;

use crate::ctx::Ctx;
use crate::error::Result;
use crate::model::{ProviderId, Snapshot};

pub async fn fetch(id: ProviderId, ctx: &Ctx) -> Result<Snapshot> {
    match id {
        ProviderId::Codex => codex::fetch(ctx).await,
        ProviderId::Claude => claude::fetch(ctx).await,
        ProviderId::Gemini => gemini::fetch(ctx).await,
        ProviderId::Copilot => copilot::fetch(ctx).await,
        ProviderId::Opencode => opencode::fetch(ctx).await,
        ProviderId::Cursor => cursor::fetch(ctx).await,
        ProviderId::Jetbrains => jetbrains::fetch(ctx).await,
        ProviderId::Codebuff => codebuff::fetch(ctx).await,
        ProviderId::Openrouter => keyed::openrouter(ctx).await,
        ProviderId::Deepseek => keyed::deepseek(ctx).await,
        ProviderId::Moonshot => keyed::moonshot(ctx).await,
        ProviderId::Zai => keyed::zai(ctx).await,
        ProviderId::Minimax => keyed::minimax(ctx).await,
    }
}
