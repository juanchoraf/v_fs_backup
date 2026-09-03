fn main() {
    if let Err(error) = v_fs_backup::run_from_env() {
        v_fs_backup::print_error(&*error);
        std::process::exit(1);
    }
}
