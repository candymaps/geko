#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// geko alone: every module comes from `GEKO_MODULES` or is downloaded.
fn main() -> anyhow::Result<()> {
    geko_app::main(Vec::new())
}
