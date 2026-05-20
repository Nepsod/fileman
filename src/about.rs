pub struct AboutInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub authors: &'static str,
    pub license: &'static str,
}

pub const ABOUT: AboutInfo = AboutInfo {
    name: env!("CARGO_PKG_NAME"),
    version: env!("CARGO_PKG_VERSION"),
    description: env!("CARGO_PKG_DESCRIPTION"),
    authors: env!("CARGO_PKG_AUTHORS"),
    license: env!("CARGO_PKG_LICENSE"),
};

pub const REPOSITORY: &str = "https://github.com/Nepsod/fileman";
