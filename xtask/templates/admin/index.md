# Admin Commands

These are all the admin commands. TODO fill me out

{%~ for category in categories %}
- [`!admin {{ category.name }}`]({{ category.name }}/) {{ category.description }}
{%- endfor %}
