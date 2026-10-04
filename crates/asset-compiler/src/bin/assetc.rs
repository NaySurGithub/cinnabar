fn main() -> Result<(), Box<dyn std::error::Error>> {
    asset_compiler::run_args(std::env::args_os())
}
