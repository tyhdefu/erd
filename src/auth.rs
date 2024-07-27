struct Auth {}

impl Auth {
    fn get_login(&self) -> Login {}
}

struct Login {
    username: String,
    /// Password or Token
    password: String,
}
