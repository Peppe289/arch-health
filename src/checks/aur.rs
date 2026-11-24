//! AUR updates: foreign packages (from libalpm) are looked up with the AUR
//! RPC v5 `info` endpoint and compared with `alpm::vercmp`, like paru/yay.

use std::cmp::Ordering;
use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::net;
use crate::report::{CheckResult, Status};

const RPC_URL: &str = "https://aur.archlinux.org/rpc/v5/info";
/// Keep requests well below the AUR's per-request argument limit.
const CHUNK: usize = 150;

#[derive(Deserialize)]
struct RpcResponse {
    #[serde(rename = "type")]
    kind: String,
    error: Option<String>,
    #[serde(default)]
    results: Vec<AurPkg>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AurPkg {
    name: String,
    version: String,
    out_of_date: Option<i64>,
    maintainer: Option<String>,
}

fn query(agent: &ureq::Agent, names: &[&str]) -> Result<Vec<AurPkg>> {
    let form = names.iter().map(|n| ("arg[]", *n));
    let resp: RpcResponse = agent
        .post(RPC_URL)
        .send_form(form)
        .context("richiesta alla AUR RPC")?
        .body_mut()
        .read_json()
        .context("risposta della AUR RPC non valida")?;
    if resp.kind == "error" {
        bail!("AUR RPC: {}", resp.error.unwrap_or_default());
    }
    Ok(resp.results)
}

/// `-debug` split packages are produced locally by makepkg and never exist
/// in the AUR: compare them with their base package instead.
fn missing_note(name: &str, local: &str, foreign: &[(String, String)]) -> String {
    let base = name
        .strip_suffix("-debug")
        .and_then(|b| foreign.iter().find(|(n, _)| n == b));
    match base {
        Some((base, base_ver)) if base_ver != local => {
            format!("{name}: pacchetto di debug ({local}) non allineato a {base} {base_ver}, rimuovibile")
        }
        Some((base, _)) => format!("{name}: pacchetto di debug di {base}"),
        None => format!("{name}: non presente in AUR (pacchetto locale o rimosso)"),
    }
}

/// `foreign`: (name, installed version) of packages not found in sync repos.
pub fn check(foreign: &[(String, String)]) -> Vec<CheckResult> {
    match check_inner(foreign) {
        Ok(r) => r,
        Err(e) => vec![CheckResult::error("Aggiornamenti AUR", e)],
    }
}

fn check_inner(foreign: &[(String, String)]) -> Result<Vec<CheckResult>> {
    if foreign.is_empty() {
        return Ok(vec![CheckResult::new("Aggiornamenti AUR", Status::Ok, "nessun pacchetto esterno")]);
    }
    let agent = net::agent();
    let names: Vec<&str> = foreign.iter().map(|(n, _)| n.as_str()).collect();
    let mut aur: HashMap<String, AurPkg> = HashMap::new();
    for chunk in names.chunks(CHUNK) {
        aur.extend(query(&agent, chunk)?.into_iter().map(|p| (p.name.clone(), p)));
    }

    let (mut updates, mut notes) = (Vec::new(), Vec::new());
    for (name, local) in foreign {
        let Some(remote) = aur.get(name) else {
            notes.push(missing_note(name, local, foreign));
            continue;
        };
        if alpm::vercmp(remote.version.as_str(), local.as_str()) == Ordering::Greater {
            updates.push(format!("{name} {local} → {}", remote.version));
        }
        if remote.out_of_date.is_some() {
            notes.push(format!("{name}: segnalato come obsoleto (out-of-date) in AUR"));
        }
        if remote.maintainer.is_none() {
            notes.push(format!("{name}: orfano in AUR (senza maintainer)"));
        }
    }

    let upd = if updates.is_empty() {
        CheckResult::new("Aggiornamenti AUR", Status::Ok, format!("{} pacchetti AUR aggiornati", aur.len()))
    } else {
        CheckResult::new("Aggiornamenti AUR", Status::Warn, format!("{} aggiornamenti AUR disponibili", updates.len()))
            .with_details(updates)
    };
    let mut out = vec![upd];
    if !notes.is_empty() {
        out.push(
            CheckResult::new("Stato AUR", Status::Info, format!("{} segnalazioni sui pacchetti AUR", notes.len()))
                .with_details(notes),
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::missing_note;

    #[test]
    fn debug_packages_compare_with_base() {
        let foreign = vec![("paru".to_string(), "2.2.1-1".to_string())];
        assert!(missing_note("paru-debug", "2.1.0-2", &foreign).contains("non allineato"));
        assert!(missing_note("paru-debug", "2.2.1-1", &foreign).contains("pacchetto di debug di paru"));
        assert!(missing_note("foo", "1-1", &foreign).contains("non presente in AUR"));
    }
}
