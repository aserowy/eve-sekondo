//! KPI cards: a name, a value, and optionally a detail line and a link. Every page renders them
//! through the `kpi_card.html` partial.

pub struct KpiLink {
    pub href: String,
    pub label: String,
    /// Draws the log-in icon in front of the label.
    pub log_in_icon: bool,
}

impl KpiLink {
    pub fn new(href: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            href: href.into(),
            label: label.into(),
            log_in_icon: false,
        }
    }

    /// Where a character that lacks a scope logs in again to grant it.
    pub fn log_in_again() -> Self {
        Self {
            href: "/auth/add".to_owned(),
            label: "Log in again".to_owned(),
            log_in_icon: true,
        }
    }
}

pub struct Kpi {
    pub id: Option<String>,
    pub name: String,
    pub value: String,
    /// The exact value, shown on hover.
    pub exact: String,
    pub detail: Option<String>,
    /// What hovering the detail line adds, when that is more than the line says.
    pub detail_title: Option<String>,
    pub link: Option<KpiLink>,
    pub error: bool,
}

impl Kpi {
    pub fn new(
        name: impl Into<String>,
        value: impl Into<String>,
        exact: impl Into<String>,
    ) -> Self {
        Self {
            id: None,
            name: name.into(),
            value: value.into(),
            exact: exact.into(),
            detail: None,
            detail_title: None,
            link: None,
            error: false,
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_detail_title(mut self, title: impl Into<String>) -> Self {
        self.detail_title = Some(title.into());
        self
    }

    pub fn with_link(mut self, link: KpiLink) -> Self {
        self.link = Some(link);
        self
    }

    pub fn as_error(mut self) -> Self {
        self.error = true;
        self
    }
}
