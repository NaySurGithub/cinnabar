fn main() -> Result<(), Box<dyn std::error::Error>> {
    asset_compiler::run_args(std::env::args_os()).or_else(|error| {
        match error.downcast::<clap::Error>() {
            Ok(error) => error.exit(),
            Err(error) => Err(error),
        }
    })
}
