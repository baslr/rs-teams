//! Browser-based login: open Chrome → Teams → auto-login via SSO.
//! Intercept network requests and extract the chatsvcagg token from localStorage.
//! Teams uses its own Chat Service API, not Microsoft Graph, for chat operations.

use chrono::{DateTime, TimeZone, Utc};
use headless_chrome::browser::LaunchOptions;
use headless_chrome::Browser;
use std::io::Write;
use std::thread;
use std::time::{Duration, Instant};

use crate::utils::time;

use crate::error::AppError;

const LOGIN_TIMEOUT: Duration = Duration::from_secs(180);
const POLL_INTERVAL: Duration = Duration::from_millis(2000);

const DEBUG_LOG: &str = "browser_debug.log";

fn debug_log(msg: &str) {
    let timestamp = time::get_now_formatted();
    let line = format!("[{timestamp}] {msg}\n");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(DEBUG_LOG)
    {
        let _ = f.write_all(line.as_bytes());
    }
    tracing::debug!("{msg}");
}

/// A chat folder from Teams IndexedDB (conversation-folder-manager)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatFolder {
    pub id: String,
    pub name: String,
    pub folder_type: String,
    pub sort_type: String,
    pub is_expanded: bool,
    pub is_deleted: bool,
    pub version: u64,
    pub conversation_ids: Vec<String>,
}

/// What we extract from the Teams browser session
#[derive(Debug, Clone)]
pub struct BrowserSession {
    /// Bearer token for api.spaces.skype.com (used for /api/mt/ middle-tier calls)
    pub skype_spaces_token: String,
    /// Bearer token for ic3.teams.office.com (used for /api/chatsvc/ chat operations)
    pub ic3_token: String,
    /// Bearer token for graph.microsoft.com (used for /me profile)
    pub graph_token: String,
    /// The region prefix (e.g. "de")
    pub region: String,
    /// The middle-tier region (e.g. "emea")
    pub mt_region: String,
    /// The full chat service URL (e.g. "https://de-prod.asyncgw.teams.microsoft.com")
    pub chat_service_url: String,
    /// Expiry of the skype_spaces token
    pub expires_at: DateTime<Utc>,
    /// User's display name
    pub display_name: String,
    /// User's object ID
    pub user_id: String,
    /// Bearer token for chatsvcagg.teams.microsoft.com (used for /api/csa/ folder operations)
    pub csa_token: String,
    /// Cookies from teams.microsoft.com
    pub cookies: Vec<(String, String)>,
    /// Chat folders from IndexedDB (conversation-folder-manager)
    pub chat_folders: Vec<ChatFolder>,
    /// Folder display order (folder IDs in order)
    pub folder_order: Vec<String>,
}

pub fn login_with_browser_sync() -> Result<BrowserSession, AppError> {
    let _ = std::fs::write(DEBUG_LOG, "");
    debug_log("=== Browser login started ===");

    let options = LaunchOptions::default_builder()
        .headless(false)
        .window_size(Some((1280, 900)))
        .idle_browser_timeout(LOGIN_TIMEOUT + Duration::from_secs(60))
        .build()
        .map_err(|e| AppError::Auth(format!("Chrome launch options failed: {e}")))?;

    let browser = Browser::new(options)
        .map_err(|e| AppError::Auth(format!("Could not start Chrome: {e}")))?;

    let tab = browser
        .new_tab()
        .map_err(|e| AppError::Auth(format!("Could not open tab: {e}")))?;

    // Install fetch/XHR/WebSocket interceptor to capture all API calls
    let js_intercept = r#"
    (function() {
        if (window.__teams_rs_intercepted) return;
        window.__teams_rs_intercepted = true;
        window.__teams_rs_requests = [];
        window.__teams_rs_websockets = [];

        // Intercept fetch
        var origFetch = window.fetch;
        window.fetch = function(url, opts) {
            var entry = {
                url: typeof url === 'string' ? url : (url.url || ''),
                method: (opts && opts.method) || 'GET',
                headers: {}
            };
            if (opts && opts.headers) {
                if (opts.headers instanceof Headers) {
                    opts.headers.forEach(function(v, k) { entry.headers[k] = v; });
                } else if (typeof opts.headers === 'object') {
                    for (var k in opts.headers) { entry.headers[k] = String(opts.headers[k]); }
                }
            }
            // Only keep API calls with authorization
            if (entry.headers['authorization'] || entry.headers['Authorization']) {
                window.__teams_rs_requests.push(entry);
            }
            return origFetch.apply(this, arguments);
        };

        // Intercept WebSocket
        var OrigWebSocket = window.WebSocket;
        window.WebSocket = function(url, protocols) {
            window.__teams_rs_websockets.push({
                url: url,
                protocols: protocols,
                openedAt: Date.now()
            });
            if (protocols) {
                return new OrigWebSocket(url, protocols);
            }
            return new OrigWebSocket(url);
        };
        window.WebSocket.prototype = OrigWebSocket.prototype;
        window.WebSocket.CONNECTING = OrigWebSocket.CONNECTING;
        window.WebSocket.OPEN = OrigWebSocket.OPEN;
        window.WebSocket.CLOSING = OrigWebSocket.CLOSING;
        window.WebSocket.CLOSED = OrigWebSocket.CLOSED;

        // Intercept XMLHttpRequest
        var origXHROpen = XMLHttpRequest.prototype.open;
        var origXHRSetHeader = XMLHttpRequest.prototype.setRequestHeader;
        var origXHRSend = XMLHttpRequest.prototype.send;
        XMLHttpRequest.prototype.open = function(method, url) {
            this.__teams_rs_method = method;
            this.__teams_rs_url = url;
            this.__teams_rs_headers = {};
            return origXHROpen.apply(this, arguments);
        };
        XMLHttpRequest.prototype.setRequestHeader = function(name, value) {
            if (this.__teams_rs_headers) {
                this.__teams_rs_headers[name] = value;
            }
            return origXHRSetHeader.apply(this, arguments);
        };
        XMLHttpRequest.prototype.send = function() {
            if (this.__teams_rs_headers &&
                (this.__teams_rs_headers['authorization'] || this.__teams_rs_headers['Authorization'])) {
                window.__teams_rs_requests.push({
                    url: this.__teams_rs_url,
                    method: this.__teams_rs_method,
                    headers: this.__teams_rs_headers,
                    type: 'xhr'
                });
            }
            return origXHRSend.apply(this, arguments);
        };
    })()
    "#;

    let teams_url = "https://teams.microsoft.com";
    debug_log(&format!("Navigating to {teams_url}"));
    tab.navigate_to(teams_url)
        .map_err(|e| AppError::Auth(format!("Navigation failed: {e}")))?;

    thread::sleep(Duration::from_millis(500));
    let _ = tab.evaluate(js_intercept, true);

    debug_log(&format!(
        "Waiting for login (max {}s)...",
        LOGIN_TIMEOUT.as_secs()
    ));
    let deadline = Instant::now() + LOGIN_TIMEOUT;

    loop {
        if Instant::now() > deadline {
            return Err(AppError::Auth(format!(
                "Login timeout after {}s",
                LOGIN_TIMEOUT.as_secs()
            )));
        }

        thread::sleep(POLL_INTERVAL);
        let _ = tab.evaluate(js_intercept, true);

        let current_url = tab.get_url().to_lowercase();
        if !current_url.contains("teams.microsoft.com") {
            debug_log(&format!("Not on Teams yet: {current_url}"));
            continue;
        }

        match try_extract_session(&tab) {
            Ok(session) => {
                debug_log(&format!(
                    "Session extracted! user={}, region={}, expires={}",
                    session.display_name, session.region, session.expires_at
                ));

                // Wait for Teams to finish loading
                debug_log("Waiting 10s for Teams to finish loading...");
                thread::sleep(Duration::from_secs(10));

                // Re-inject interceptor in case page navigated
                let _ = tab.evaluate(js_intercept, true);

                // IndexedDB folder scraping disabled — folders now come from CSA API
                // let (chat_folders, folder_order) = extract_chat_folders(&tab);
                // session.chat_folders = chat_folders;
                // session.folder_order = folder_order;

                // TEST: Make a direct fetch call from the browser to test the API
                // Use the chatsvcagg token (not ic3) for conversations endpoint
                test_api_from_browser(&tab, &session);

                // Also dump the SKYPE-TOKEN entry specifically
                dump_skype_token_detail(&tab);

                // Dump intercepted chat API calls for debugging
                dump_chat_api_calls(&tab);

                drop(tab);
                drop(browser);
                return Ok(session);
            }
            Err(e) => {
                debug_log(&format!("Session not ready yet: {e}"));
            }
        }
    }
}

/// Extract the api.spaces.skype.com token + graph token + user info from localStorage
fn try_extract_session(tab: &headless_chrome::Tab) -> Result<BrowserSession, AppError> {
    let js = r#"
    (function() {
        try {
            var keys = Object.keys(localStorage);
            var spacesToken = null, spacesExpires = 0;
            var ic3Token = null;
            var csaToken = null;
            var graphToken = null, graphExpires = 0;
            var region = '';
            var mtRegion = '';
            var chatServiceUrl = '';
            var displayName = '';
            var userId = '';
            var allKeys = [];

            for (var i = 0; i < keys.length; i++) {
                var key = keys[i];
                var val = localStorage.getItem(key);
                if (!val) continue;

                allKeys.push(key);

                // api.spaces.skype.com token from MSAL (for /api/mt/ middle-tier)
                if (key.toLowerCase().indexOf('-accesstoken-') !== -1 &&
                    key.toLowerCase().indexOf('api.spaces.skype.com') !== -1) {
                    try {
                        var obj = JSON.parse(val);
                        if (obj.secret && obj.credentialType === 'AccessToken') {
                            spacesToken = obj.secret;
                            spacesExpires = parseInt(obj.expiresOn) || 0;
                        }
                    } catch(e) {}
                }

                // graph token from MSAL
                if (key.toLowerCase().indexOf('-accesstoken-') !== -1 &&
                    key.toLowerCase().indexOf('graph.microsoft.com') !== -1) {
                    try {
                        var obj = JSON.parse(val);
                        if (obj.secret && obj.credentialType === 'AccessToken') {
                            graphToken = obj.secret;
                            graphExpires = parseInt(obj.expiresOn) || 0;
                        }
                    } catch(e) {}
                }

                // ic3 token from MSAL (for /api/chatsvc/ chat operations)
                if (key.toLowerCase().indexOf('-accesstoken-') !== -1 &&
                    key.toLowerCase().indexOf('ic3.teams.office.com') !== -1) {
                    try {
                        var obj = JSON.parse(val);
                        if (obj.secret && obj.credentialType === 'AccessToken') {
                            ic3Token = obj.secret;
                        }
                    } catch(e) {}
                }

                // chatsvcagg token from MSAL (for /api/csa/ folder operations)
                if (key.toLowerCase().indexOf('-accesstoken-') !== -1 &&
                    key.toLowerCase().indexOf('chatsvcagg') !== -1) {
                    try {
                        var obj = JSON.parse(val);
                        if (obj.secret && obj.credentialType === 'AccessToken') {
                            csaToken = obj.secret;
                        }
                    } catch(e) {}
                }

                // Region from discovery data
                if (key.indexOf('Discover.DISCOVER-REGION-GTM') !== -1) {
                    try {
                        var obj = JSON.parse(val);
                        if (obj.item && obj.item.chatService) {
                            var cs = obj.item.chatService;
                            chatServiceUrl = cs;
                            var regionMatch = cs.match(/https?:\/\/([a-z]{2})[.-]/);
                            if (regionMatch) {
                                region = regionMatch[1];
                            }
                        }
                        if (obj.item && obj.item.regionGtms) {
                            var gtms = obj.item.regionGtms;
                            if (gtms.amsV2) {
                                chatServiceUrl = gtms.amsV2;
                            } else if (gtms.ams) {
                                chatServiceUrl = gtms.ams;
                            }
                        }
                    } catch(e) {}
                }

                // User details
                if (key.indexOf('Discover.DISCOVER-USER-DETAILS') !== -1) {
                    try {
                        var obj = JSON.parse(val);
                        if (obj.item) {
                            userId = obj.item.id || '';
                        }
                    } catch(e) {}
                }

                // Display name from user profile
                if (key.indexOf('GLOBAL.User.User') !== -1) {
                    try {
                        var obj = JSON.parse(val);
                        if (obj.item && obj.item.profile) {
                            displayName = obj.item.profile.name || '';
                        }
                    } catch(e) {}
                }
            }

            // Detect mt_region from intercepted requests (e.g. /api/mt/emea/beta/...)
            var reqs = window.__teams_rs_requests || [];
            for (var j = 0; j < reqs.length; j++) {
                var url = reqs[j].url;
                var mtMatch = url.match(/\/api\/mt\/([a-z]+)\//);
                if (mtMatch) {
                    mtRegion = mtMatch[1]; // e.g. "emea"
                    break;
                }
            }
            // Also detect region from chatsvc requests
            if (!region) {
                for (var j = 0; j < reqs.length; j++) {
                    var url = reqs[j].url;
                    var csMatch = url.match(/\/api\/chatsvc\/([a-z]{2})\//);
                    if (csMatch) {
                        region = csMatch[1];
                        break;
                    }
                }
            }

            if (!spacesToken || !ic3Token) {
                return JSON.stringify({found: false, reason: 'Missing tokens: spaces=' + !!spacesToken + ' ic3=' + !!ic3Token + ', keys: ' + keys.length, allKeys: allKeys});
            }

            if (!mtRegion) mtRegion = 'emea';
            if (!region) region = 'de';

            return JSON.stringify({
                found: true,
                spaces_token: spacesToken,
                spaces_expires: spacesExpires,
                ic3_token: ic3Token || '',
                csa_token: csaToken || '',
                graph_token: graphToken || '',
                graph_expires: graphExpires,
                region: region,
                mt_region: mtRegion,
                chat_service_url: chatServiceUrl,
                display_name: displayName,
                user_id: userId,
                all_keys: allKeys
            });
        } catch(e) {
            return JSON.stringify({found: false, reason: e.toString()});
        }
    })()
    "#;

    let result = tab
        .evaluate(js, true)
        .map_err(|e| AppError::Auth(format!("JS eval failed: {e}")))?;

    let json_str = result
        .value
        .as_ref()
        .and_then(|v| v.as_str())
        .unwrap_or("{}");

    let parsed: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| AppError::Auth(format!("Parse failed: {e}")))?;

    if !parsed
        .get("found")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let reason = parsed
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        if let Some(keys) = parsed.get("allKeys").or(parsed.get("all_keys")) {
            debug_log(&format!("All localStorage keys: {}", keys));
        }
        return Err(AppError::Auth(reason.to_string()));
    }

    if let Some(keys) = parsed.get("all_keys") {
        debug_log(&format!("All localStorage keys: {}", keys));
    }

    let skype_spaces_token = parsed
        .get("spaces_token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let ic3_token = parsed
        .get("ic3_token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let csa_token = parsed
        .get("csa_token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let graph_token = parsed
        .get("graph_token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let region = parsed
        .get("region")
        .and_then(|v| v.as_str())
        .unwrap_or("de")
        .to_string();
    let mt_region = parsed
        .get("mt_region")
        .and_then(|v| v.as_str())
        .unwrap_or("emea")
        .to_string();
    let chat_service_url = parsed
        .get("chat_service_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let display_name = parsed
        .get("display_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let user_id = parsed
        .get("user_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let expires_ts = parsed
        .get("spaces_expires")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let expires_at = Utc
        .timestamp_opt(expires_ts, 0)
        .single()
        .unwrap_or_else(|| Utc::now() + chrono::Duration::hours(1));

    debug_log(&format!(
        "Spaces token found: {} chars",
        skype_spaces_token.len()
    ));
    debug_log(&format!("IC3 token found: {} chars", ic3_token.len()));
    debug_log(&format!("CSA token found: {} chars", csa_token.len()));
    debug_log(&format!("Region: {region}, MT region: {mt_region}"));
    debug_log(&format!("Chat service URL: {chat_service_url}"));

    // Extract cookies from the browser
    let cookies = match tab.get_cookies() {
        Ok(all_cookies) => {
            let teams_cookies: Vec<(String, String)> = all_cookies
                .iter()
                .filter(|c| {
                    c.domain.contains("teams.microsoft.com") || c.domain.contains(".microsoft.com")
                })
                .map(|c| (c.name.clone(), c.value.clone()))
                .collect();
            debug_log(&format!(
                "Extracted {} cookies from browser",
                teams_cookies.len()
            ));
            teams_cookies
        }
        Err(e) => {
            debug_log(&format!("Could not get cookies: {e}"));
            vec![]
        }
    };

    Ok(BrowserSession {
        skype_spaces_token,
        ic3_token,
        csa_token,
        graph_token,
        region,
        mt_region,
        chat_service_url,
        expires_at,
        display_name,
        user_id,
        cookies,
        chat_folders: vec![],
        folder_order: vec![],
    })
}

/// Dump intercepted requests that go to the chat service
fn dump_chat_api_calls(tab: &headless_chrome::Tab) {
    let js = r#"
    (function() {
        var reqs = window.__teams_rs_requests || [];
        var chatReqs = reqs.filter(function(r) {
            return r.url.indexOf('/api/chatsvc/') !== -1 ||
                   r.url.indexOf('chatsvcagg') !== -1;
        });
        // Show all headers for these calls
        return JSON.stringify(chatReqs.slice(0, 30), null, 2);
    })()
    "#;

    if let Ok(result) = tab.evaluate(js, true) {
        let text = result
            .value
            .as_ref()
            .and_then(|v| v.as_str())
            .unwrap_or("[]");
        debug_log("=== CHAT SERVICE API CALLS ===");
        debug_log(text);
        debug_log("=== END CHAT SERVICE API CALLS ===");
    }

    // Dump WebSocket connections
    let js_ws = r#"
    (function() {
        var ws = window.__teams_rs_websockets || [];
        return JSON.stringify(ws, null, 2);
    })()
    "#;

    if let Ok(result) = tab.evaluate(js_ws, true) {
        let text = result
            .value
            .as_ref()
            .and_then(|v| v.as_str())
            .unwrap_or("[]");
        debug_log("=== WEBSOCKET CONNECTIONS ===");
        debug_log(text);
        debug_log("=== END WEBSOCKET CONNECTIONS ===");
    }

    // Also dump ALL api calls for full picture
    let js_all = r#"
    (function() {
        var reqs = window.__teams_rs_requests || [];
        var apiReqs = reqs.filter(function(r) {
            return r.url.indexOf('/api/') !== -1 ||
                   r.url.indexOf('graph.microsoft.com') !== -1;
        });
        // Just URLs and methods
        return apiReqs.map(function(r) {
            return r.method + ' ' + r.url;
        }).join('\n');
    })()
    "#;

    if let Ok(result) = tab.evaluate(js_all, true) {
        let text = result
            .value
            .as_ref()
            .and_then(|v| v.as_str())
            .unwrap_or("(none)");
        debug_log("=== ALL API CALL URLs ===");
        debug_log(text);
        debug_log("=== END ALL API CALL URLs ===");
    }
}

/// Test the middle-tier API directly from the browser context
fn test_api_from_browser(tab: &headless_chrome::Tab, session: &BrowserSession) {
    let mt_region = &session.mt_region;
    let token = &session.skype_spaces_token;

    // Check what transport Teams uses: WebSocket, XHR, fetch, etc.
    let js = r#"
    (function() {
        var results = {};

        // Check for WebSocket connections
        if (window.__teams_rs_websockets) {
            results.websockets = window.__teams_rs_websockets.map(function(ws) {
                return { url: ws.url, readyState: ws.readyState };
            });
        } else {
            results.websockets = 'not intercepted yet';
        }

        // Check for active Service Workers
        if (navigator.serviceWorker && navigator.serviceWorker.controller) {
            results.serviceWorker = navigator.serviceWorker.controller.scriptURL;
        } else {
            results.serviceWorker = 'none';
        }

        // Check for SharedWorker
        results.hasSharedWorker = typeof SharedWorker !== 'undefined';

        // Look for trouter/long-polling connections in intercepted requests
        var reqs = window.__teams_rs_requests || [];
        var trouterReqs = reqs.filter(function(r) {
            return r.url.indexOf('trouter') !== -1 ||
                   r.url.indexOf('poll') !== -1 ||
                   r.url.indexOf('socket') !== -1 ||
                   r.url.indexOf('signalr') !== -1;
        });
        results.trouterRequests = trouterReqs.map(function(r) { return r.method + ' ' + r.url; });

        // Check for EventSource (SSE)
        results.hasEventSource = typeof EventSource !== 'undefined';

        return JSON.stringify(results, null, 2);
    })()
    "#;

    if let Ok(result) = tab.evaluate(js, true) {
        let text = result
            .value
            .as_ref()
            .and_then(|v| v.as_str())
            .unwrap_or("{}");
        debug_log("=== TRANSPORT DETECTION ===");
        debug_log(text);
        debug_log("=== END TRANSPORT DETECTION ===");
    }
}

/// Dump the SKYPE-TOKEN entry in detail
fn dump_skype_token_detail(tab: &headless_chrome::Tab) {
    let js = r#"
    (function() {
        var keys = Object.keys(localStorage);
        for (var i = 0; i < keys.length; i++) {
            if (keys[i].indexOf('SKYPE-TOKEN') !== -1) {
                var val = localStorage.getItem(keys[i]);
                try {
                    var obj = JSON.parse(val);
                    // Get the skypeToken value length
                    var token = obj.item && obj.item.skypeToken ? obj.item.skypeToken : '';
                    return JSON.stringify({
                        key: keys[i],
                        hasToken: token.length > 0,
                        tokenLength: token.length,
                        tokenPrefix: token.substring(0, 20),
                        expiration: obj.item && obj.item.expiration
                    });
                } catch(e) {
                    return JSON.stringify({key: keys[i], error: e.toString()});
                }
            }
        }
        return JSON.stringify({found: false});
    })()
    "#;

    if let Ok(result) = tab.evaluate(js, true) {
        let text = result
            .value
            .as_ref()
            .and_then(|v| v.as_str())
            .unwrap_or("{}");
        debug_log(&format!("SKYPE-TOKEN detail: {text}"));
    }
}

/// Dump the values of tmp.auth.v1.*.Token.* and Discover.SKYPE-TOKEN entries
fn dump_auth_tokens(tab: &headless_chrome::Tab) {
    let js = r#"
    (function() {
        var keys = Object.keys(localStorage);
        var tokens = {};
        for (var i = 0; i < keys.length; i++) {
            var key = keys[i];
            if (key.indexOf('.Token.') !== -1 || key.indexOf('SKYPE-TOKEN') !== -1) {
                var val = localStorage.getItem(key);
                try {
                    var obj = JSON.parse(val);
                    // Truncate long token values for readability
                    var summary = {};
                    for (var k in obj) {
                        var v = obj[k];
                        if (typeof v === 'object' && v !== null) {
                            var innerSummary = {};
                            for (var ik in v) {
                                var iv = v[ik];
                                if (typeof iv === 'string' && iv.length > 50) {
                                    innerSummary[ik] = iv.substring(0, 50) + '...[' + iv.length + ' chars]';
                                } else {
                                    innerSummary[ik] = iv;
                                }
                            }
                            summary[k] = innerSummary;
                        } else if (typeof v === 'string' && v.length > 50) {
                            summary[k] = v.substring(0, 50) + '...[' + v.length + ' chars]';
                        } else {
                            summary[k] = v;
                        }
                    }
                    tokens[key] = summary;
                } catch(e) {
                    tokens[key] = val && val.length > 100 ? val.substring(0, 100) + '...' : val;
                }
            }
        }
        return JSON.stringify(tokens, null, 2);
    })()
    "#;

    if let Ok(result) = tab.evaluate(js, true) {
        let text = result
            .value
            .as_ref()
            .and_then(|v| v.as_str())
            .unwrap_or("{}");
        debug_log("=== AUTH TOKENS (tmp.auth.v1.*.Token.*) ===");
        debug_log(text);
        debug_log("=== END AUTH TOKENS ===");
    }
}

/// Extract chat folders from IndexedDB (conversation-folder-manager database).
///
/// Teams stores all folder data in IndexedDB via the SharedWorker.
/// We read the `folders` store for folder records and `folders-internal-data`
/// for the display order (FolderOrder record).
fn extract_chat_folders(tab: &headless_chrome::Tab) -> (Vec<ChatFolder>, Vec<String>) {
    let js = r#"
    (async function() {
        try {
            var dbs = await indexedDB.databases();
            var folderDb = dbs.find(function(db) {
                return db.name && db.name.indexOf('conversation-folder-manager') !== -1;
            });
            if (!folderDb) {
                return JSON.stringify({error: 'no folder db', dbNames: dbs.map(function(d){return d.name;})});
            }

            var db = await new Promise(function(resolve, reject) {
                var req = indexedDB.open(folderDb.name);
                req.onsuccess = function() { resolve(req.result); };
                req.onerror = function() { reject(req.error); };
            });

            var storeNames = Array.from(db.objectStoreNames);
            var result = {folders: [], folderOrder: []};

            // Read folders store
            if (storeNames.indexOf('folders') !== -1) {
                var tx = db.transaction('folders', 'readonly');
                var store = tx.objectStore('folders');
                var allData = await new Promise(function(resolve, reject) {
                    var req = store.getAll();
                    req.onsuccess = function() { resolve(req.result); };
                    req.onerror = function() { reject(req.error); };
                });
                result.folders = allData;
            }

            // Read folder order from folders-internal-data
            if (storeNames.indexOf('folders-internal-data') !== -1) {
                var tx2 = db.transaction('folders-internal-data', 'readonly');
                var store2 = tx2.objectStore('folders-internal-data');
                var allInternal = await new Promise(function(resolve, reject) {
                    var req = store2.getAll();
                    req.onsuccess = function() { resolve(req.result); };
                    req.onerror = function() { reject(req.error); };
                });
                for (var i = 0; i < allInternal.length; i++) {
                    if (allInternal[i].id === 'FolderOrder' && allInternal[i].folderIds) {
                        result.folderOrder = allInternal[i].folderIds;
                        break;
                    }
                }
            }

            db.close();
            return JSON.stringify(result);
        } catch(e) {
            return JSON.stringify({error: e.message});
        }
    })()
    "#;

    match tab.evaluate(js, true) {
        Ok(r) => {
            let json_str = r.value.as_ref().and_then(|v| v.as_str()).unwrap_or("{}");
            debug_log(&format!(
                "IDB folder raw response: {} bytes",
                json_str.len()
            ));

            match serde_json::from_str::<serde_json::Value>(json_str) {
                Ok(parsed) => {
                    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
                        debug_log(&format!("IDB folder error: {}", err));
                        return (vec![], vec![]);
                    }
                    parse_folder_idb_response(&parsed)
                }
                Err(e) => {
                    debug_log(&format!("IDB folder parse error: {}", e));
                    (vec![], vec![])
                }
            }
        }
        Err(e) => {
            debug_log(&format!("IDB folder eval error: {}", e));
            (vec![], vec![])
        }
    }
}

/// Parse the JSON response from the IndexedDB folder query into ChatFolder structs.
///
/// The JSON has this shape:
/// ```json
/// {
///   "folders": [
///     {
///       "id": "uuid",
///       "name": "MyFolder",
///       "folderType": "UserCreated",
///       "sortType": "UserDefinedCustomOrder",
///       "isExpanded": true,
///       "isDeleted": false,
///       "version": 1775588983643,
///       "conversations": [{"id": "19:...", "threadType": "chat"}, ...]
///     }, ...
///   ],
///   "folderOrder": ["uuid1", "uuid2", ...]
/// }
/// ```
fn parse_folder_idb_response(parsed: &serde_json::Value) -> (Vec<ChatFolder>, Vec<String>) {
    let mut folders = Vec::new();

    if let Some(folder_arr) = parsed.get("folders").and_then(|v| v.as_array()) {
        for f in folder_arr {
            let id = f
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = f
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let folder_type = f
                .get("folderType")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let sort_type = f
                .get("sortType")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let is_expanded = f
                .get("isExpanded")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let is_deleted = f
                .get("isDeleted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let version = f.get("version").and_then(|v| v.as_u64()).unwrap_or(0);

            let conversation_ids: Vec<String> = f
                .get("conversations")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| c.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            if !id.is_empty() {
                folders.push(ChatFolder {
                    id,
                    name,
                    folder_type,
                    sort_type,
                    is_expanded,
                    is_deleted,
                    version,
                    conversation_ids,
                });
            }
        }
    }

    let folder_order: Vec<String> = parsed
        .get("folderOrder")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    (folders, folder_order)
}

/// Async wrapper
pub async fn login_with_browser() -> Result<BrowserSession, AppError> {
    tokio::task::spawn_blocking(login_with_browser_sync)
        .await
        .map_err(|e| AppError::Auth(format!("Login task panicked: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_user_created_folder() {
        let json = serde_json::json!({
            "folders": [{
                "id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
                "name": "MyFolder",
                "folderType": "UserCreated",
                "sortType": "UserDefinedCustomOrder",
                "isExpanded": true,
                "isDeleted": false,
                "version": 1775588983643u64,
                "conversations": [
                    {"id": "19:abc@thread.v2", "threadType": "chat"},
                    {"id": "19:def@thread.v2", "threadType": "meeting"}
                ]
            }],
            "folderOrder": ["bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"]
        });

        let (folders, order) = parse_folder_idb_response(&json);

        assert_eq!(folders.len(), 1);
        let f = &folders[0];
        assert_eq!(f.id, "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
        assert_eq!(f.name, "MyFolder");
        assert_eq!(f.folder_type, "UserCreated");
        assert_eq!(f.sort_type, "UserDefinedCustomOrder");
        assert!(f.is_expanded);
        assert!(!f.is_deleted);
        assert_eq!(f.version, 1775588983643);
        assert_eq!(
            f.conversation_ids,
            vec!["19:abc@thread.v2", "19:def@thread.v2"]
        );
        assert_eq!(order, vec!["bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"]);
    }

    #[test]
    fn parse_system_folder() {
        let json = serde_json::json!({
            "folders": [{
                "id": "tenant~user~Favorites",
                "name": "Favorites",
                "folderType": "Favorites",
                "sortType": "UserDefinedCustomOrder",
                "isExpanded": false,
                "isDeleted": false,
                "version": 123,
                "conversations": []
            }],
            "folderOrder": []
        });

        let (folders, order) = parse_folder_idb_response(&json);

        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].folder_type, "Favorites");
        assert_eq!(folders[0].conversation_ids.len(), 0);
        assert!(order.is_empty());
    }

    #[test]
    fn parse_multiple_folders_with_order() {
        let json = serde_json::json!({
            "folders": [
                {
                    "id": "aaa",
                    "name": "MyFolder",
                    "folderType": "UserCreated",
                    "sortType": "UserDefinedCustomOrder",
                    "isExpanded": true,
                    "isDeleted": false,
                    "version": 1,
                    "conversations": [{"id": "19:chat1@thread.v2", "threadType": "chat"}]
                },
                {
                    "id": "bbb",
                    "name": "Archived",
                    "folderType": "UserCreated",
                    "sortType": "UserDefinedCustomOrder",
                    "isExpanded": false,
                    "isDeleted": false,
                    "version": 2,
                    "conversations": []
                },
                {
                    "id": "ccc",
                    "name": "MeetingChats",
                    "folderType": "MeetingChats",
                    "sortType": "MostRecent",
                    "isExpanded": true,
                    "isDeleted": false,
                    "version": 3,
                    "conversations": []
                }
            ],
            "folderOrder": ["ccc", "aaa", "bbb"]
        });

        let (folders, order) = parse_folder_idb_response(&json);

        assert_eq!(folders.len(), 3);
        assert_eq!(folders[0].name, "MyFolder");
        assert_eq!(folders[1].name, "Archived");
        assert_eq!(folders[2].name, "MeetingChats");
        assert_eq!(order, vec!["ccc", "aaa", "bbb"]);
    }

    #[test]
    fn parse_empty_response() {
        let json = serde_json::json!({
            "folders": [],
            "folderOrder": []
        });

        let (folders, order) = parse_folder_idb_response(&json);

        assert!(folders.is_empty());
        assert!(order.is_empty());
    }

    #[test]
    fn parse_error_response_returns_empty() {
        let json = serde_json::json!({
            "error": "no folder db"
        });

        // parse_folder_idb_response doesn't handle errors — that's done in the caller.
        // But it should return empty when folders/folderOrder keys are missing.
        let (folders, order) = parse_folder_idb_response(&json);

        assert!(folders.is_empty());
        assert!(order.is_empty());
    }

    #[test]
    fn parse_folder_missing_conversations_key() {
        let json = serde_json::json!({
            "folders": [{
                "id": "abc",
                "name": "NoConvs",
                "folderType": "UserCreated",
                "sortType": "MostRecent",
                "isExpanded": false,
                "isDeleted": true,
                "version": 99
            }],
            "folderOrder": ["abc"]
        });

        let (folders, _) = parse_folder_idb_response(&json);

        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "NoConvs");
        assert!(folders[0].is_deleted);
        assert!(folders[0].conversation_ids.is_empty());
    }

    #[test]
    fn parse_folder_skips_empty_id() {
        let json = serde_json::json!({
            "folders": [
                {"id": "", "name": "Empty", "folderType": "UserCreated", "sortType": "MostRecent",
                 "isExpanded": false, "isDeleted": false, "version": 0},
                {"id": "valid", "name": "Valid", "folderType": "UserCreated", "sortType": "MostRecent",
                 "isExpanded": false, "isDeleted": false, "version": 1}
            ],
            "folderOrder": []
        });

        let (folders, _) = parse_folder_idb_response(&json);

        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].id, "valid");
    }
}
