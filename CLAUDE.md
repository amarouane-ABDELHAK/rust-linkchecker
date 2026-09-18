<!-- dev-collab-playbook:begin -->
## Collaboration playbook

This project runs on the
[dev-collab-playbook](https://github.com/NASA-IMPACT/dev-collab-playbook)
plugin, enabled in `.claude/settings.json`. The plugin injects the routing
table and the hard gates at session start and re-checks `vision.md` on
every prompt; the text it injects is `hooks/playbook-context.md` in the
plugin. If no `dev-collab-playbook:*` skills are available in this session,
the plugin is not loaded: trusting the project folder registers the
marketplace but does not install a plugin served from a repository, so tell
the developer to run
`claude plugin install dev-collab-playbook@dev-collab-playbook`, and stop.

### Project anchors

- `vision.md` — goals, audience, non-goals, direction. Review objections
  cite this or an intent note, never personal taste.
- `docs/intent/` — one note per nontrivial task. `TEMPLATE.md` is the shape.
- `docs/decisions/` — one record per settled dispute. `TEMPLATE.md` is the
  shape. Link the record when the topic reappears.
<!-- dev-collab-playbook:end -->
