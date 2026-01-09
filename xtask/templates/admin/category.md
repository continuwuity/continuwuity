# `!admin {{ name }}`

{{ description }}

{%  for command in commands %}
{% let header = "#".repeat((command.depth + 1).min(3)) -%}
{{ header }} `!admin {{ command.name }}`

{{ command.description }}
{% endfor %}
