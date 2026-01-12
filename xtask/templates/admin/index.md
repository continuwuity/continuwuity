# Admin Command Reference

Admin commands allow server administrators to manage the server from within their Matrix client. "Server administrators" by default means only those users which are members of the admin room, but additional server admins may be added using the `admins_list` configuration option.

## Running commands

* All commands listed here may be used by server administrators in the admin room by sending them as messages.
* If the `admin_escape_commands` configuration option is enabled, server administrators may run certain commands in public rooms by prefixing them with a single backslash. These commands will only run on _their_ homeserver, even if they are a member of another homeserver's admin room. Some sensitive commands cannot be used outside the admin room and will return an error.
* All commands listed here may be used in the server's console, if it is enabled. Commands entered in the console do not require the `!admin` prefix.

## Categories

{%~ for category in categories %}
- [`!admin {{ category.name }}`]({{ category.name }}/) {{ category.description }}
{%- endfor %}
