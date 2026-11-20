pub mod agy;
pub mod bailian;
pub mod claude;
pub mod codebuff;
pub mod codex;
pub mod commandcode;
pub mod copilot;
pub mod cursor;
pub mod grok;
pub mod jetbrains;
pub mod keyed;
pub mod kiro;
pub mod opencode;
pub mod qianfan;
pub mod qoder;
pub mod volcengine;
pub mod workbuddy;

use crate::ctx::Ctx;
use crate::error::Result;
use crate::model::{ProviderId, Snapshot};

pub async fn fetch(id: ProviderId, ctx: &Ctx) -> Result<Snapshot> {
    match id {
        ProviderId::Codex => codex::fetch(ctx).await,
        ProviderId::Claude => claude::fetch(ctx).await,
        ProviderId::Agy => agy::fetch(ctx).await,
        ProviderId::Grok => grok::fetch(ctx).await,
        ProviderId::Copilot => copilot::fetch(ctx).await,
        ProviderId::Opencode => opencode::fetch(ctx).await,
        ProviderId::Cursor => cursor::fetch(ctx).await,
        ProviderId::Jetbrains => jetbrains::fetch(ctx).await,
        ProviderId::Codebuff => codebuff::fetch(ctx).await,
        ProviderId::Workbuddy => workbuddy::fetch(ctx).await,
        ProviderId::Kiro => kiro::fetch(ctx).await,
        ProviderId::Commandcode => commandcode::fetch(ctx).await,
        ProviderId::Openrouter => keyed::openrouter(ctx).await,
        ProviderId::Deepseek => keyed::deepseek(ctx).await,
        ProviderId::Moonshot => keyed::moonshot(ctx).await,
        ProviderId::Zai => keyed::zai(ctx).await,
        ProviderId::Minimax => keyed::minimax(ctx).await,
        ProviderId::Volcengine => volcengine::fetch(ctx).await,
        ProviderId::Bailian => bailian::fetch(ctx).await,
        ProviderId::Qianfan => qianfan::fetch(ctx).await,
        ProviderId::Qoder => qoder::fetch(ctx).await,
    }
}
