use crate::Engine;
use std::path::Path;
use stratum_domain::*;
impl Engine {
    pub fn applications(&self) -> Result<Vec<Application>> {
        self.cached("applications", || self.discover_applications())
    }
    fn discover_applications(&self) -> Result<Vec<Application>> {
        let bundles = self.files(&FileQuery {
            kind: Some("directory".into()),
            name: Some("*.app".into()),
            limit: 1000,
            ..Default::default()
        })?;
        let mut result = vec![];
        for e in bundles.items {
            if Path::new(&e.parent)
                .ancestors()
                .any(|p| p.extension().is_some_and(|x| x == "app"))
            {
                continue;
            }
            let info = plist::Value::from_file(Path::new(&e.path).join("Contents/Info.plist")).ok();
            if info.as_ref().and_then(|v| v.as_dictionary()).is_none() {
                continue;
            }
            let bundle_id = info
                .as_ref()
                .and_then(|v| v.as_dictionary())
                .and_then(|d| d.get("CFBundleIdentifier"))
                .and_then(|v| v.as_string())
                .map(str::to_owned);
            let name = info
                .as_ref()
                .and_then(|v| v.as_dictionary())
                .and_then(|d| {
                    d.get("CFBundleDisplayName")
                        .or_else(|| d.get("CFBundleName"))
                })
                .and_then(|v| v.as_string())
                .map(str::to_owned)
                .unwrap_or_else(|| e.name.trim_end_matches(".app").into());
            let mut associations = vec![Association {
                path: e.path.clone(),
                kind: "application_bundle".into(),
                bytes: e.logical_bytes,
                confidence: "confirmed".into(),
                evidence: vec![Evidence::new(
                    "bundle_directory",
                    "Indexed .app bundle; size includes nested bundle files",
                )],
                selected_by_default: true,
            }];
            if let Some(bundle) = &bundle_id {
                // Only exact bundle-identifier names are evidence; never match arbitrary prefixes.
                for expected in [
                    bundle.clone(),
                    format!("{bundle}.plist"),
                    format!("{bundle}.savedState"),
                ] {
                    for item in self
                        .files(&FileQuery {
                            name: Some(expected.clone()),
                            limit: 1000,
                            ..Default::default()
                        })?
                        .items
                        .into_iter()
                        .filter(|i| i.name == expected)
                    {
                        if item.path.starts_with(&format!("{}/", e.path)) {
                            continue;
                        }
                        let parent = Path::new(&item.parent);
                        let area = parent.file_name().and_then(|s| s.to_str()).unwrap_or("");
                        if ![
                            "Application Support",
                            "Caches",
                            "Preferences",
                            "Logs",
                            "Containers",
                            "Group Containers",
                            "Saved Application State",
                        ]
                        .contains(&area)
                        {
                            continue;
                        }
                        associations.push(Association{path:item.path,kind:area.into(),bytes:item.logical_bytes,confidence:"high".into(),evidence:vec![Evidence::new("bundle_identifier_match",format!("Exact name {expected} in {area}; data may be shared by application versions"))],selected_by_default:area!="Group Containers"});
                    }
                }
            }
            result.push(Application{id:blake3::hash(e.path.as_bytes()).to_hex().to_string(),name,bundle_id,path:e.path,footprint_bytes:associations.iter().map(|a|a.bytes).sum(),associations,coverage:"Estimate from scanned paths only; shared data, plugins and unscanned Library locations may be missing".into()});
        }
        result.sort_by_key(|a| std::cmp::Reverse(a.footprint_bytes));
        Ok(result)
    }
    pub fn inspect_application(&self, id: &str) -> Result<Application> {
        self.applications()?
            .into_iter()
            .find(|a| a.id == id || a.name == id)
            .ok_or_else(|| Error::new("not_found", "Application not found in indexed bundles"))
    }
    pub fn uninstall_plan(&self, id: &str) -> Result<serde_json::Value> {
        let app = self.inspect_application(id)?;
        let plan_id = domain::id();
        let proposal = serde_json::json!({"id":plan_id,"application":app,"action":"uninstall_review","executable":false,"reason":"Bundle and application-data removal are not supported by the regular-file quarantine executor. Review associations; no data has been changed."});
        self.store.put("uninstall_plan", &plan_id, &proposal)?;
        self.store.audit("uninstall_plan_created", &plan_id, id)?;
        Ok(proposal)
    }
}
use crate::domain;
