use serde::{Deserialize, Serialize};

use crate::helpers;

helpers::socket_addr_config! {
    MetricsServiceConfig,
    9_000,
    "metrics_service"
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct MetricsConfig {
    #[serde(default = "helpers::serde_defaults::bool_true")]
    pub enabled: bool,
    #[serde(default)]
    #[serde(flatten)]
    pub service: MetricsServiceConfig,
}