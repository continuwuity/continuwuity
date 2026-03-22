## Thank you for trying out Continuwuity!

Your new homeserver is ready to use! {%- if config.allow_federation %} To make sure you can federate with the rest of the Matrix network, consider checking your domain (`{{ domain }}`) with a federation tester like [this one](https://connectivity-tester.mtrnord.blog/). {%- endif %}

{% if config.get_config_file_token().is_some() -%}
Users may now create accounts normally using the configured registration token.
{%- else if config.recaptcha_site_key.is_some() -%}
Users may now create accounts normally after solving a CAPTCHA.
{%- else if config.yes_i_am_very_very_sure_i_want_an_open_registration_server_prone_to_abuse -%}
**This server has open, unrestricted registration enabled!** Anyone, including spammers, may now create an account with no further steps. If this is not desired behavior, set `yes_i_am_very_very_sure_i_want_an_open_registration_server_prone_to_abuse` to `false` in your configuration and restart the server.
{%- else if config.allow_registration -%}
To allow more users to register, use the `!admin token` admin commands to issue registration tokens, or set a registration token in the configuration.
{%- else -%}
You've disabled registration. To create more accounts, use the `!admin users create-user` admin command.
{%- endif %}

This room is your server's admin room. You can send messages starting with `!admin` in this room to perform a range of administrative actions.
To view a list of available commands, send the following message: `!admin --help`

Project chatrooms:
> Support chatroom: https://matrix.to/#/#continuwuity:continuwuity.org
> Update announcements: https://matrix.to/#/#announcements:continuwuity.org
> Other chatrooms: https://matrix.to/#/#space:continuwuity.org
>

Helpful links:
> Source code: https://forgejo.ellis.link/continuwuation/continuwuity
> Documentation: https://continuwuity.org/
> Report issues: https://forgejo.ellis.link/continuwuation/continuwuity/issues
