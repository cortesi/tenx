|:-|:-
| comment | {{comment}} 
|-
{% match error %}
  {% when Some with (val) %}| *error* | {{ val }} 
|-
  {% when None %}{% else %}
{% endmatch %}