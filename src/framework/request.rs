//! Plain-data views of serenity interactions. Serenity's interaction types are
//! `#[non_exhaustive]`, so features take these instead, and tests can build them directly.

use std::collections::HashMap;

use serenity::all::{
    ActionRowComponent, CommandDataOption, CommandDataOptionValue, CommandInteraction,
    ComponentInteraction, ComponentInteractionDataKind, GuildId, ModalInteraction, UserId,
};

/// A slash command invocation, with subcommands already unwrapped.
#[derive(Debug, Clone)]
pub struct CommandRequest {
    pub user: UserId,
    pub guild: Option<GuildId>,
    /// Top-level command name, e.g. `ss`.
    pub command: String,
    /// Subcommand group name, if the command uses groups.
    pub group: Option<String>,
    /// Subcommand name, e.g. `info`.
    pub subcommand: Option<String>,
    /// Options of the innermost (sub)command.
    pub options: Options,
}

impl CommandRequest {
    pub fn new(user: UserId, command: impl Into<String>) -> Self {
        Self {
            user,
            guild: None,
            command: command.into(),
            group: None,
            subcommand: None,
            options: Options::default(),
        }
    }

    pub fn subcommand(mut self, name: impl Into<String>) -> Self {
        self.subcommand = Some(name.into());
        self
    }

    pub fn option(mut self, name: impl Into<String>, value: CommandDataOptionValue) -> Self {
        self.options.0.insert(name.into(), value);
        self
    }
}

impl From<&CommandInteraction> for CommandRequest {
    fn from(command: &CommandInteraction) -> Self {
        let (group, subcommand, options) = flatten(&command.data.options);
        Self {
            user: command.user.id,
            guild: command.guild_id,
            command: command.data.name.clone(),
            group,
            subcommand,
            options,
        }
    }
}

fn flatten(options: &[CommandDataOption]) -> (Option<String>, Option<String>, Options) {
    match options.first().map(|o| (&o.name, &o.value)) {
        Some((name, CommandDataOptionValue::SubCommandGroup(inner))) => {
            let (_, subcommand, options) = flatten(inner);
            (Some(name.clone()), subcommand, options)
        }
        Some((name, CommandDataOptionValue::SubCommand(inner))) => {
            (None, Some(name.clone()), Options::from_data(inner))
        }
        _ => (None, None, Options::from_data(options)),
    }
}

/// Named option values of a command.
#[derive(Debug, Clone, Default)]
pub struct Options(HashMap<String, CommandDataOptionValue>);

impl Options {
    fn from_data(options: &[CommandDataOption]) -> Self {
        Self(
            options
                .iter()
                .map(|o| (o.name.clone(), o.value.clone()))
                .collect(),
        )
    }

    pub fn i64(&self, name: &str) -> Option<i64> {
        self.0.get(name)?.as_i64()
    }

    pub fn str(&self, name: &str) -> Option<&str> {
        self.0.get(name)?.as_str()
    }

    pub fn user(&self, name: &str) -> Option<UserId> {
        match self.0.get(name)? {
            CommandDataOptionValue::User(id) => Some(*id),
            _ => None,
        }
    }
}

/// A button press or select-menu submission.
#[derive(Debug, Clone)]
pub struct ComponentRequest {
    pub user: UserId,
    pub custom_id: String,
    pub kind: ComponentKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentKind {
    Button,
    UserSelect(Vec<UserId>),
    StringSelect(Vec<String>),
    Other,
}

impl ComponentRequest {
    pub fn button(user: UserId, custom_id: impl Into<String>) -> Self {
        Self {
            user,
            custom_id: custom_id.into(),
            kind: ComponentKind::Button,
        }
    }

    pub fn user_select(user: UserId, custom_id: impl Into<String>, values: Vec<UserId>) -> Self {
        Self {
            user,
            custom_id: custom_id.into(),
            kind: ComponentKind::UserSelect(values),
        }
    }
}

impl From<&ComponentInteraction> for ComponentRequest {
    fn from(component: &ComponentInteraction) -> Self {
        let kind = match &component.data.kind {
            ComponentInteractionDataKind::Button => ComponentKind::Button,
            ComponentInteractionDataKind::UserSelect { values } => {
                ComponentKind::UserSelect(values.clone())
            }
            ComponentInteractionDataKind::StringSelect { values } => {
                ComponentKind::StringSelect(values.clone())
            }
            _ => ComponentKind::Other,
        };
        Self {
            user: component.user.id,
            custom_id: component.data.custom_id.clone(),
            kind,
        }
    }
}

/// A submitted modal, with its text inputs keyed by custom id.
#[derive(Debug, Clone)]
pub struct ModalRequest {
    pub user: UserId,
    pub custom_id: String,
    pub fields: HashMap<String, String>,
}

impl ModalRequest {
    pub fn new<'a>(
        user: UserId,
        custom_id: impl Into<String>,
        fields: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Self {
        Self {
            user,
            custom_id: custom_id.into(),
            fields: fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    pub fn field(&self, id: &str) -> Option<&str> {
        self.fields.get(id).map(String::as_str)
    }
}

impl From<&ModalInteraction> for ModalRequest {
    fn from(modal: &ModalInteraction) -> Self {
        let fields = modal
            .data
            .components
            .iter()
            .flat_map(|row| row.components.iter())
            .filter_map(|component| match component {
                ActionRowComponent::InputText(input) => {
                    Some((input.custom_id.clone(), input.value.clone()?))
                }
                _ => None,
            })
            .collect();
        Self {
            user: modal.user.id,
            custom_id: modal.data.custom_id.clone(),
            fields,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(json: serde_json::Value) -> CommandInteraction {
        let mut base = serde_json::json!({
            "id": "1", "application_id": "2", "type": 2, "token": "t", "version": 1, "channel_id": "5",
            "user": { "id": "42", "username": "u", "discriminator": "0" },
            "locale": "en-US", "app_permissions": "0", "entitlements": [],
            "authorizing_integration_owners": {},
        });
        base["data"] = json;
        serde_json::from_value(base).unwrap()
    }

    #[test]
    fn flattens_subcommand_options() {
        let interaction = command(serde_json::json!({
            "id": "3", "name": "ss", "type": 1,
            "options": [{ "name": "info", "type": 1, "options": [
                { "name": "id", "type": 4, "value": 7 }
            ]}]
        }));

        let req = CommandRequest::from(&interaction);

        assert_eq!(req.command, "ss");
        assert_eq!(req.user, UserId::new(42));
        assert_eq!(req.group, None);
        assert_eq!(req.subcommand.as_deref(), Some("info"));
        assert_eq!(req.options.i64("id"), Some(7));
    }

    #[test]
    fn flattens_subcommand_groups() {
        let interaction = command(serde_json::json!({
            "id": "3", "name": "cfg", "type": 1,
            "options": [{ "name": "user", "type": 2, "options": [
                { "name": "set", "type": 1, "options": [
                    { "name": "value", "type": 3, "value": "x" }
                ]}
            ]}]
        }));

        let req = CommandRequest::from(&interaction);

        assert_eq!(req.group.as_deref(), Some("user"));
        assert_eq!(req.subcommand.as_deref(), Some("set"));
        assert_eq!(req.options.str("value"), Some("x"));
    }

    #[test]
    fn top_level_options_without_subcommand() {
        let interaction = command(serde_json::json!({
            "id": "3", "name": "ping", "type": 1,
            "options": [{ "name": "count", "type": 4, "value": 2 }]
        }));

        let req = CommandRequest::from(&interaction);

        assert_eq!(req.subcommand, None);
        assert_eq!(req.options.i64("count"), Some(2));
    }
}
