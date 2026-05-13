#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitMessage<'a> {
    pub header: Header<'a>,
    pub body: Option<&'a str>,
    pub footers: Vec<Footer<'a>>,
    pub breaking: bool,
    pub breaking_description: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header<'a> {
    pub ty: &'a str,
    pub scope: Option<&'a str>,
    pub breaking: bool,
    pub description: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Footer<'a> {
    pub token: &'a str,
    pub separator: FooterSeparator,
    pub value: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FooterSeparator {
    Colon,
    Hash,
}

impl<'a> CommitMessage<'a> {
    pub fn ty(&self) -> &'a str {
        self.header.ty
    }

    pub fn is_breaking(&self) -> bool {
        self.breaking
    }
}

impl Footer<'_> {
    pub fn is_breaking_change(&self) -> bool {
        is_breaking_token(self.token)
    }
}

pub(crate) fn is_breaking_token(token: &str) -> bool {
    matches!(token, "BREAKING CHANGE" | "BREAKING-CHANGE")
}
