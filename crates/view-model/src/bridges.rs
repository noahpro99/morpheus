//! Bridges (#107): connecting other networks (Discord, WhatsApp, Telegram,
//! Signal, ...) through the mautrix bridges. A mautrix bridge is run by a bot
//! user on the homeserver that is talked to by direct message in commands
//! (`login`, `ping`, `logout`) and answers with text and, for a QR code, an
//! image. This module knows the networks and their bots, reads the bot's
//! replies, and runs the login as a small state machine; the app only sends
//! what [`Login`] says to send and feeds it each reply. No toolkit in here.
//!
//! The replies are matched by their words (case ignored), as written by
//! mautrix-whatsapp, -signal, -telegram, -discord and the bridgev2 core
//! the newer ones share, so a bridge that words a line a little differently
//! still reads right.

use morpheus_core::RoomSummary;

/// A network a bridge brings in, with what to tell someone linking it.
#[derive(Debug, PartialEq, Eq)]
pub struct Network {
    /// The id the bridges' `m.bridge` state gives (`discord`).
    pub id: &'static str,
    pub name: &'static str,
    /// The bridge bot's localpart on a server that runs it (`discordbot`).
    pub bot: &'static str,
    /// The login flows to prefer, in order, when the bot offers several.
    pub flows: &'static [&'static str],
    /// How to link it, in words (what to open on the phone).
    pub steps: &'static str,
}

pub const NETWORKS: &[Network] = &[
    Network {
        id: "whatsapp",
        name: "WhatsApp",
        bot: "whatsappbot",
        flows: &["qr"],
        steps: "Open WhatsApp on your phone, go to Settings, Linked devices, Link a device, and scan the code below.",
    },
    Network {
        id: "signal",
        name: "Signal",
        bot: "signalbot",
        flows: &["qr"],
        steps: "Open Signal on your phone, go to Settings, Linked devices, add a new device, and scan the code below.",
    },
    Network {
        id: "discord",
        name: "Discord",
        bot: "discordbot",
        flows: &["qr"],
        steps: "Open Discord on your phone, go to You, Scan QR Code, and scan the code below.",
    },
    Network {
        id: "telegram",
        name: "Telegram",
        bot: "telegrambot",
        flows: &["phone", "qr"],
        steps: "Enter your phone number with its country code, then the code Telegram sends you (and your password if you set one).",
    },
    Network {
        id: "slack",
        name: "Slack",
        bot: "slackbot",
        flows: &["token", "qr"],
        steps: "Follow the bridge's questions to sign in to your Slack workspace.",
    },
    Network {
        id: "instagram",
        name: "Instagram",
        bot: "instagrambot",
        flows: &["username", "qr"],
        steps: "Enter your Instagram username and password when the bridge asks.",
    },
    Network {
        id: "messenger",
        name: "Messenger",
        bot: "metabot",
        flows: &["messenger", "qr"],
        steps: "Follow the bridge's questions to sign in to Facebook Messenger.",
    },
    Network {
        id: "googlechat",
        name: "Google Chat",
        bot: "googlechatbot",
        flows: &["cookies", "qr"],
        steps: "Follow the bridge's questions to sign in to Google Chat.",
    },
    Network {
        id: "gmessages",
        name: "Google Messages",
        bot: "gmessagesbot",
        flows: &["qr"],
        steps: "Open Google Messages on your phone, go to Device pairing, scan the QR code, and scan the code below.",
    },
    Network {
        id: "imessage",
        name: "iMessage",
        bot: "imessagebot",
        flows: &["qr"],
        steps: "Follow the bridge's questions to link iMessage.",
    },
    Network {
        id: "twitter",
        name: "Twitter",
        bot: "twitterbot",
        flows: &["cookies", "qr"],
        steps: "Follow the bridge's questions to sign in to Twitter.",
    },
    Network {
        id: "bluesky",
        name: "Bluesky",
        bot: "blueskybot",
        flows: &["password", "qr"],
        steps: "Enter your handle and an app password when the bridge asks.",
    },
    Network {
        id: "linkedin",
        name: "LinkedIn",
        bot: "linkedinbot",
        flows: &["cookies", "qr"],
        steps: "Follow the bridge's questions to sign in to LinkedIn.",
    },
];

/// What to say when the bridge's own network is not one listed.
/// The words of this module the screens translate by name (the status words, each network's steps).
pub fn translatable_labels() -> Vec<&'static str> {
    let mut labels: Vec<&'static str> = NETWORKS.iter().map(|n| n.steps).collect();
    labels.push(GENERIC_STEPS);
    labels.extend([
        "Looking for bridges...",
        "No bridge bots were found on your server. If your bridge's bot has another name, add it below.",
        "Log in again",
        "Type your answer",
        "Bot name, e.g. discordbot or @bot:example.org",
    ]);
    labels.extend(["Not connected", "Checking", "Logged out", "Connecting", "Logged in", "Needs login", "Error"]);
    labels
}

pub const GENERIC_STEPS: &str =
    "Follow the bridge's instructions below. A QR code is shown here when the bridge sends one.";

pub fn network(id: &str) -> Option<&'static Network> {
    let id = id.trim().to_lowercase();
    NETWORKS.iter().find(|n| n.id == id)
}

/// The network a bot user ID belongs to, from its localpart (`@discordbot:x`).
pub fn network_of_bot(bot: &str) -> Option<&'static Network> {
    let local = bot.trim().trim_start_matches('@').split(':').next()?.to_lowercase();
    NETWORKS
        .iter()
        .find(|n| local == n.bot || local.contains(n.id) || (n.id == "messenger" && local.contains("meta")))
}

/// The bot's user ID for a typed name: a bare localpart (`discordbot`) is on
/// `home`; `@name:server` or `name:server` is taken as it is.
pub fn normalise_bot(typed: &str, home: &str) -> Option<String> {
    let typed = typed.trim().trim_start_matches('@').to_lowercase();
    if typed.is_empty() || typed.contains(char::is_whitespace) {
        return None;
    }
    let (local, server) = match typed.split_once(':') {
        Some((local, server)) => (local, server),
        None => (typed.as_str(), home),
    };
    if local.is_empty() || server.is_empty() || server.contains(['@', '/']) || local.contains(['@', ':', '/']) {
        return None;
    }
    Some(format!("@{local}:{server}"))
}

/// A bot to look for on the homeserver: its user ID and the network it is
/// for (`None` for one typed in by hand that is not one listed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub bot: String,
    pub network: Option<&'static str>,
}

/// The bots worth asking the homeserver about: the well-known name of each
/// network on `home`, then the ones named by `extra` (typed in, saved, or the
/// bot a bridged room names), without repeats.
pub fn candidates(home: &str, extra: &[String]) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = NETWORKS
        .iter()
        .map(|n| Candidate { bot: format!("@{}:{home}", n.bot), network: Some(n.id) })
        .collect();
    for typed in extra {
        let Some(bot) = normalise_bot(typed, home) else {
            continue;
        };
        if out.iter().any(|c| c.bot == bot) {
            continue;
        }
        out.push(Candidate { network: network_of_bot(&bot).map(|n| n.id), bot });
    }
    out
}

/// The bot IDs the rooms of a person name (`m.bridge`'s `bridgebot`), for
/// finding bridges set up under other names.
pub fn bots_in_rooms(bridges: &[morpheus_core::Bridge]) -> Vec<String> {
    let mut bots: Vec<String> = Vec::new();
    for bot in bridges.iter().filter_map(|b| b.bot.clone()) {
        if !bots.contains(&bot) {
            bots.push(bot);
        }
    }
    bots
}

/// The network id a room belongs to: its bridge's, or, for a direct message
/// with a bridge's ghost user, the one that user's name starts with.
pub fn network_id_of(room: &RoomSummary) -> Option<String> {
    if let Some(bridge) = &room.bridge {
        return Some(bridge.protocol.clone());
    }
    let user = room.dm_user.as_ref()?;
    morpheus_core::network_of_ghost(user).map(str::to_owned)
}

/// What a network is called: the well-known name, else the bridge's own.
pub fn network_name(rooms: &[RoomSummary], id: &str) -> String {
    if let Some(n) = network(id) {
        return n.name.to_owned();
    }
    rooms
        .iter()
        .find_map(|r| r.bridge.as_ref().filter(|b| b.protocol == id).map(|b| b.name.clone()))
        .unwrap_or_else(|| id.to_owned())
}

/// The networks the rooms are bridged to, each once, in the order of the
/// list above, then others by name.
pub fn networks_in(rooms: &[RoomSummary]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for room in rooms.iter().filter(|r| !r.invited && !r.is_space) {
        if let Some(id) = network_id_of(room)
            && !ids.contains(&id)
        {
            ids.push(id);
        }
    }
    let rank = |id: &String| NETWORKS.iter().position(|n| n.id == id).unwrap_or(usize::MAX);
    ids.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
    ids
}

// ---------------------------------------------------------------- replies

/// One message from the bridge bot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub text: String,
    /// An image (the QR code to scan).
    pub image: bool,
}

/// A login flow the bot offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flow {
    pub id: String,
    pub name: String,
}

/// What a reply means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The ways to log in, to pick one.
    Flows(Vec<Flow>),
    /// A QR code to scan (an image).
    Qr,
    /// The bot asks for something typed (a phone number, a code, a password).
    Prompt(String),
    /// Logged in, as whom when it says.
    LoggedIn(Option<String>),
    /// Not logged in.
    LoggedOut,
    /// It says the login is broken and must be done again.
    NeedsLogin(String),
    /// The login or the command failed.
    Failed(String),
    /// Something else (help, progress); shown as it is.
    Info(String),
}

/// The words of a reply, lower case, for matching.
fn lower(text: &str) -> String {
    text.to_lowercase().replace('\u{2019}', "'")
}

fn has_any(text: &str, words: &[&str]) -> bool {
    words.iter().any(|w| text.contains(w))
}

/// The flows a "choose a login flow" reply lists: lines like
/// ``* `qr` - Link with a QR code``.
fn flows_in(text: &str) -> Vec<Flow> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches(['*', '-', '\u{2022}']).trim();
            let rest = line.strip_prefix('`')?;
            let (id, after) = rest.split_once('`')?;
            if id.is_empty() || id.contains(' ') {
                return None;
            }
            let name = after.trim().trim_start_matches(['-', ':', '\u{2013}']).trim();
            Some(Flow { id: id.to_owned(), name: name.to_owned() })
        })
        .collect()
}

/// What the person after "as" is, from "logged in as +1 555 (device #1)".
fn who_after_as(text: &str) -> Option<String> {
    let low = lower(text);
    let at = low.find(" as ")? + 4;
    let who = text[at..].lines().next()?.trim().trim_end_matches(['.', '!']).trim();
    let who = who.trim_matches('`').trim_matches('*');
    (!who.is_empty()).then(|| who.to_owned())
}

/// Read one reply of the bot.
pub fn parse_reply(reply: &Reply) -> Event {
    let text = reply.text.trim();
    if reply.image {
        return Event::Qr;
    }
    let low = lower(text);
    // Logged out first: "you're not logged in" contains "logged in".
    if has_any(&low, &["not logged in", "no logins", "not logged into", "you have no active", "you're logged out"]) {
        return Event::LoggedOut;
    }
    if has_any(&low, &["successfully logged out", "logged out successfully", "you've been logged out", "logged out"]) {
        return Event::LoggedOut;
    }
    if has_any(
        &low,
        &["bad_credentials", "bad credentials", "unknown_error", "logged out by", "token expired", "session expired", "re-login", "relogin", "log in again"],
    ) && !has_any(&low, &["successfully"])
    {
        return Event::NeedsLogin(text.to_owned());
    }
    if has_any(&low, &["successfully logged in", "logged in as", "already logged in", "login successful"])
        || (low.contains("connected") && !low.contains("not connected") && !low.contains("disconnected"))
    {
        return Event::LoggedIn(who_after_as(text));
    }
    // A wrong code or password the bot lets be tried again is a question, not a failure.
    if has_any(&low, &["invalid", "incorrect", "wrong"]) && has_any(&low, &["try again", "please enter", "please send", "send the"]) {
        return Event::Prompt(text.to_owned());
    }
    if has_any(
        &low,
        &["login failed", "failed to log in", "failed to login", "timed out", "time out", "login cancelled", "login canceled", "expired", "error", "failed", "invalid", "incorrect", "unknown command"],
    ) {
        return Event::Failed(text.to_owned());
    }
    let flows = flows_in(text);
    if flows.len() >= 2 || (flows.len() == 1 && has_any(&low, &["flow", "login method", "log in with", "choose"])) {
        return Event::Flows(flows);
    }
    if has_any(
        &low,
        &[
            "please enter", "please send", "please type", "please provide", "enter your", "enter the", "send your", "send the",
            "send me", "type your", "your phone number", "verification code", "two-factor", "2fa", "password", "reply with",
        ],
    ) {
        return Event::Prompt(text.to_owned());
    }
    Event::Info(text.to_owned())
}

// ------------------------------------------------------------------ login

/// Where one network's connection stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Status {
    /// Not asked yet.
    #[default]
    Unknown,
    /// Waiting for the bot's answer to the status check.
    Checking,
    LoggedOut,
    /// The login is started; the bot has not asked for anything yet.
    Starting,
    /// A QR code is shown (the latest image the bot sent).
    Qr,
    /// The bot asks for something; the question.
    Prompt(String),
    LoggedIn(Option<String>),
    /// Logged in once but the bridge needs it done again.
    NeedsLogin(String),
    Error(String),
}

impl Status {
    /// The short word for the list: what a row says.
    pub fn label(&self) -> &'static str {
        match self {
            Status::Unknown => "Not connected",
            Status::Checking => "Checking",
            Status::LoggedOut => "Logged out",
            Status::Starting | Status::Qr | Status::Prompt(_) => "Connecting",
            Status::LoggedIn(_) => "Logged in",
            Status::NeedsLogin(_) => "Needs login",
            Status::Error(_) => "Error",
        }
    }

    pub fn is_logged_in(&self) -> bool {
        matches!(self, Status::LoggedIn(_))
    }

    /// A login is under way (a QR code or a question is up).
    pub fn is_connecting(&self) -> bool {
        matches!(self, Status::Starting | Status::Qr | Status::Prompt(_))
    }
}

/// The command that asks the bot where the login stands.
pub const STATUS_COMMAND: &str = "ping";
/// The command that starts a login.
pub const LOGIN_COMMAND: &str = "login";
/// The command that ends the login.
pub const LOGOUT_COMMAND: &str = "logout";
/// Stops a login the bot is waiting on.
pub const CANCEL_COMMAND: &str = "cancel";

/// What a bridge is doing at the moment: the conversation with its bot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login {
    network: Option<&'static str>,
    pub status: Status,
    /// Logging out (so "logged out" and a list of logins are read as that).
    leaving: bool,
    /// Something typed while a flow is chosen is not an answer to a question.
    chose_flow: bool,
}

impl Login {
    pub fn new(network: Option<&'static str>) -> Self {
        Self { network, status: Status::Unknown, leaving: false, chose_flow: false }
    }

    /// The commands to send to find out where the login stands.
    pub fn check(&mut self) -> Vec<String> {
        self.status = Status::Checking;
        vec![STATUS_COMMAND.to_owned()]
    }

    /// The commands to send to log in.
    pub fn start(&mut self) -> Vec<String> {
        self.status = Status::Starting;
        self.leaving = false;
        self.chose_flow = false;
        vec![LOGIN_COMMAND.to_owned()]
    }

    /// The commands to send to log out.
    pub fn log_out(&mut self) -> Vec<String> {
        self.leaving = true;
        vec![LOGOUT_COMMAND.to_owned()]
    }

    /// Stop a login in progress.
    pub fn cancel(&mut self) -> Vec<String> {
        self.status = Status::LoggedOut;
        vec![CANCEL_COMMAND.to_owned()]
    }

    /// What was typed in answer to the bot's question (kept apart from
    /// commands: a code, a phone number); sent as it is.
    pub fn answer(&mut self, typed: &str) -> Vec<String> {
        if matches!(self.status, Status::Prompt(_)) {
            self.status = Status::Starting;
        }
        vec![typed.trim().to_owned()]
    }

    /// The flow to pick of those offered: the network's preference, else the first.
    pub fn pick_flow(&self, flows: &[Flow]) -> Option<String> {
        let prefs = self.network.and_then(network).map(|n| n.flows).unwrap_or(&["qr"]);
        for want in prefs {
            if let Some(flow) = flows.iter().find(|f| f.id == *want) {
                return Some(flow.id.clone());
            }
        }
        flows.first().map(|f| f.id.clone())
    }

    /// A reply came: the commands to send in answer, if any.
    pub fn on_reply(&mut self, reply: &Reply) -> Vec<String> {
        let event = parse_reply(reply);
        // A QR code only matters while a login is under way.
        match event {
            Event::Qr => {
                if self.status.is_connecting() || matches!(self.status, Status::Unknown | Status::Checking | Status::LoggedOut) {
                    self.status = Status::Qr;
                }
                Vec::new()
            }
            Event::Flows(flows) if self.leaving => {
                // Which login to log out of: the first it lists.
                self.leaving = false;
                match flows.first() {
                    Some(flow) => {
                        self.leaving = true;
                        vec![format!("{LOGOUT_COMMAND} {}", flow.id)]
                    }
                    None => Vec::new(),
                }
            }
            Event::Flows(flows) => match self.pick_flow(&flows) {
                Some(id) if self.status == Status::Starting && !self.chose_flow => {
                    self.chose_flow = true;
                    vec![format!("{LOGIN_COMMAND} {id}")]
                }
                _ => Vec::new(),
            },
            Event::Prompt(text) => {
                if self.status.is_connecting() {
                    self.status = Status::Prompt(text);
                }
                Vec::new()
            }
            Event::LoggedIn(who) => {
                self.leaving = false;
                self.status = Status::LoggedIn(who);
                Vec::new()
            }
            Event::LoggedOut => {
                self.leaving = false;
                // A cancelled or timed-out login says "logged out" too; nothing more to do.
                self.status = Status::LoggedOut;
                Vec::new()
            }
            Event::NeedsLogin(text) => {
                self.status = Status::NeedsLogin(text);
                Vec::new()
            }
            Event::Failed(text) => {
                // An unknown command to the status check means the bridge has no such
                // command: say so quietly, as logged out.
                if self.status == Status::Checking {
                    self.status = Status::LoggedOut;
                } else {
                    self.status = Status::Error(first_line(&text));
                }
                Vec::new()
            }
            Event::Info(_) => Vec::new(),
        }
    }
}

fn first_line(text: &str) -> String {
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str) -> Reply {
        Reply { text: t.into(), image: false }
    }

    fn qr() -> Reply {
        Reply { text: "qr.png".into(), image: true }
    }

    #[test]
    fn a_bot_is_named_by_network_and_typed_names_are_completed() {
        assert_eq!(network_of_bot("@discordbot:example.org").map(|n| n.id), Some("discord"));
        assert_eq!(network_of_bot("@mautrix_signal:x").map(|n| n.id), Some("signal"));
        assert_eq!(network_of_bot("@metabot:x").map(|n| n.id), Some("messenger"));
        assert_eq!(network_of_bot("@someone:x"), None);
        assert_eq!(normalise_bot("discordbot", "hs.org").as_deref(), Some("@discordbot:hs.org"));
        assert_eq!(normalise_bot(" @Bot:Other.org ", "hs.org").as_deref(), Some("@bot:other.org"));
        assert_eq!(normalise_bot("two words", "hs.org"), None);
        assert_eq!(normalise_bot("", "hs.org"), None);
        assert_eq!(normalise_bot("a/b", "hs.org"), None);
    }

    #[test]
    fn candidates_are_the_well_known_bots_then_typed_ones_once() {
        let list = candidates("hs.org", &["custombot".into(), "discordbot".into(), "@x:other.org".into(), "bad name".into()]);
        assert!(list.iter().any(|c| c.bot == "@whatsappbot:hs.org" && c.network == Some("whatsapp")));
        assert_eq!(list.iter().filter(|c| c.bot == "@discordbot:hs.org").count(), 1);
        let custom = list.iter().find(|c| c.bot == "@custombot:hs.org").expect("custom");
        assert_eq!(custom.network, None);
        assert!(list.iter().any(|c| c.bot == "@x:other.org"));
        assert_eq!(list.len(), NETWORKS.len() + 2);
    }

    #[test]
    fn an_image_is_the_qr_code() {
        assert_eq!(parse_reply(&qr()), Event::Qr);
    }

    #[test]
    fn success_failure_and_logged_out_are_told_apart() {
        // From mautrix-whatsapp, -signal, -telegram, -discord and bridgev2.
        assert_eq!(
            parse_reply(&text("Successfully logged in as +1 555 0100 (device #1)")),
            Event::LoggedIn(Some("+1 555 0100 (device #1)".into()))
        );
        assert_eq!(
            parse_reply(&text("Successfully logged in as Alice (+15550100)")),
            Event::LoggedIn(Some("Alice (+15550100)".into()))
        );
        assert_eq!(parse_reply(&text("You're already logged in")), Event::LoggedIn(None));
        assert_eq!(parse_reply(&text("You\u{2019}re not logged in")), Event::LoggedOut);
        assert_eq!(parse_reply(&text("You're not logged into Telegram")), Event::LoggedOut);
        assert_eq!(parse_reply(&text("Logged out successfully")), Event::LoggedOut);
        assert_eq!(parse_reply(&text("Login failed: unexpected response from server")), Event::Failed("Login failed: unexpected response from server".into()));
        assert_eq!(
            parse_reply(&text("QR code scan timed out. Please try again.")),
            Event::Failed("QR code scan timed out. Please try again.".into())
        );
        assert_eq!(
            parse_reply(&text("* `1234` (+15550100) - `BAD_CREDENTIALS`")),
            Event::NeedsLogin("* `1234` (+15550100) - `BAD_CREDENTIALS`".into())
        );
        assert_eq!(parse_reply(&text("* `1234` (+15550100) - `CONNECTED`")), Event::LoggedIn(None));
    }

    #[test]
    fn questions_are_prompts_and_a_wrong_code_is_asked_again() {
        for line in [
            "Please send your phone number here.",
            "Please enter the verification code you received",
            "Your account has two-factor authentication. Please send your password here.",
            "Enter your phone number in international format",
        ] {
            assert_eq!(parse_reply(&text(line)), Event::Prompt(line.into()), "{line}");
        }
        let again = "Invalid code. Please enter the code again";
        assert_eq!(parse_reply(&text(again)), Event::Prompt(again.into()));
    }

    #[test]
    fn a_choice_of_login_flows_is_read_from_its_list() {
        let list = "Choose a login flow:\n\n* `qr` - Link with a QR code\n* `phone` - Link with a phone number";
        match parse_reply(&text(list)) {
            Event::Flows(flows) => {
                assert_eq!(flows.len(), 2);
                assert_eq!(flows[0], Flow { id: "qr".into(), name: "Link with a QR code".into() });
                assert_eq!(flows[1].id, "phone");
            }
            other => panic!("not flows: {other:?}"),
        }
    }

    #[test]
    fn a_qr_login_runs_from_the_command_to_success() {
        let mut login = Login::new(Some("whatsapp"));
        assert_eq!(login.start(), ["login"]);
        assert_eq!(login.status, Status::Starting);
        let sent = login.on_reply(&text("Choose a login flow:\n* `qr` - QR code\n* `phone` - Phone number"));
        assert_eq!(sent, ["login qr"]);
        // The code arrives, then again when it is refreshed.
        assert!(login.on_reply(&qr()).is_empty());
        assert_eq!(login.status, Status::Qr);
        login.on_reply(&qr());
        assert_eq!(login.status, Status::Qr);
        login.on_reply(&text("Successfully logged in as +15550100"));
        assert_eq!(login.status, Status::LoggedIn(Some("+15550100".into())));
        assert_eq!(login.status.label(), "Logged in");
    }

    #[test]
    fn a_phone_login_asks_questions_and_sends_the_answers() {
        let mut login = Login::new(Some("telegram"));
        login.start();
        // Telegram prefers the phone flow over the QR one.
        let sent = login.on_reply(&text("Choose a login flow:\n* `qr` - QR code\n* `phone` - Phone number"));
        assert_eq!(sent, ["login phone"]);
        login.on_reply(&text("Please send your phone number here."));
        assert_eq!(login.status, Status::Prompt("Please send your phone number here.".into()));
        assert_eq!(login.answer(" +15550100 "), ["+15550100"]);
        assert_eq!(login.status, Status::Starting);
        login.on_reply(&text("Please send the code you received."));
        assert!(matches!(login.status, Status::Prompt(_)));
        login.answer("12345");
        login.on_reply(&text("Successfully logged in as @alice (+15550100)"));
        assert!(login.status.is_logged_in());
    }

    #[test]
    fn a_failed_login_says_why_and_a_status_check_is_quiet_when_not_logged_in() {
        let mut login = Login::new(None);
        login.start();
        login.on_reply(&qr());
        login.on_reply(&text("Login failed: QR code scan timed out"));
        assert_eq!(login.status, Status::Error("Login failed: QR code scan timed out".into()));
        assert_eq!(login.status.label(), "Error");

        let mut login = Login::new(Some("signal"));
        assert_eq!(login.check(), ["ping"]);
        login.on_reply(&text("You're not logged in"));
        assert_eq!(login.status, Status::LoggedOut);
        assert_eq!(login.check(), ["ping"]);
        login.on_reply(&text("Unknown command, use the `help` command for help."));
        assert_eq!(login.status, Status::LoggedOut);
    }

    #[test]
    fn a_broken_login_needs_doing_again_and_logging_out_answers_a_list_of_logins() {
        let mut login = Login::new(Some("discord"));
        login.check();
        login.on_reply(&text("* `1234` (alice) - `BAD_CREDENTIALS`"));
        assert!(matches!(login.status, Status::NeedsLogin(_)));
        assert_eq!(login.log_out(), ["logout"]);
        let sent = login.on_reply(&text("Please specify which login to log out of:\n\n* `1234` - alice"));
        assert!(sent.is_empty() || sent == ["logout 1234"]);
    }

    fn room(id: &str) -> RoomSummary {
        crate::test_room::room(id, id)
    }

    #[test]
    fn a_room_belongs_to_its_bridges_network_or_its_ghosts() {
        let mut bridged = room("!a:x");
        bridged.bridge = Some(morpheus_core::Bridge { protocol: "discord".into(), name: "Discord".into(), bot: Some("@discordbot:x".into()) });
        let mut dm = room("!b:x");
        dm.is_dm = true;
        dm.dm_user = morpheus_core::OwnedUserId::try_from("@whatsapp_1555:x").ok();
        let mut plain = room("!c:x");
        plain.is_dm = true;
        plain.dm_user = morpheus_core::OwnedUserId::try_from("@bob:x").ok();
        let mut other = room("!d:x");
        other.bridge = Some(morpheus_core::Bridge { protocol: "zulip".into(), name: "Zulip".into(), bot: None });
        let all = [bridged, dm, plain, other];
        assert_eq!(network_id_of(&all[0]).as_deref(), Some("discord"));
        assert_eq!(network_id_of(&all[1]).as_deref(), Some("whatsapp"));
        assert_eq!(network_id_of(&all[2]), None);
        assert_eq!(networks_in(&all), ["whatsapp", "discord", "zulip"]);
        assert_eq!(network_name(&all, "zulip"), "Zulip");
        assert_eq!(network_name(&all, "discord"), "Discord");
        let bridges: Vec<_> = all.iter().filter_map(|r| r.bridge.clone()).collect();
        assert_eq!(bots_in_rooms(&bridges), ["@discordbot:x"]);
    }

    #[test]
    fn a_bridge_event_names_its_network_and_bot() {
        let content = serde_json::json!({
            "bridgebot": "@discordbot:example.org",
            "protocol": {"id": "Discord", "displayname": "Discord"},
            "channel": {"id": "1"}
        });
        let bridge = morpheus_core::bridge_of_content(&content).expect("bridge");
        assert_eq!(bridge.protocol, "discord");
        assert_eq!(bridge.bot.as_deref(), Some("@discordbot:example.org"));
        assert!(morpheus_core::bridge_of_content(&serde_json::json!({"protocol": {}})).is_none());
    }
}
