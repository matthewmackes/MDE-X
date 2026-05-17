# Contributing to Mackes Shell

Thanks for thinking about contributing. Mackes Shell is a small,
focused project — one GTK3 Python control panel that replaces
`xfce4-settings` on Fedora — and we keep the contribution flow simple.

## Ways to help

| If you want to | Do this |
|---|---|
| Report a bug | Open an issue with the **Bug** template |
| Request a feature | Open an issue with the **Feature** template |
| Send a code patch | Open a PR against `main` |
| Improve docs | PR against `docs/help/*.md` or `README.md` |
| Add a curated playbook | PR against `data/ansible/playbooks/roles/` |
| Add a curated app | PR against `mackes/app_mgmt.py:CATALOG` |
| Add a preset | PR against `data/presets/*.yaml` |

## Development setup

```bash
git clone git@github.com:matthewmackes/MAP2-RELEASES.git
cd MAP2-RELEASES
# System deps
sudo dnf install python3-gobject gtk3 xfconf python3-pyyaml \
    ansible-core podman tomcat
# (optional) editable install
pip install -e . --user
# Smoke test
python3 -c "import mackes; print(mackes.__version__)"
# Run the wizard
python3 -m mackes --wizard
# Run the headless TUI (without an X session)
python3 -m mackes --tui
```

## Building the RPM locally

```bash
make rpm
ls rpmbuild/RPMS/x86_64/
```

## Project conventions

- **Python style:** `from __future__ import annotations` at the top of
  every module. Standard library first, third-party second,
  `mackes.*` third.
- **GTK panels:** all new panels follow the Carbon refresh layout —
  breadcrumb + page title + section helpers. See
  `mackes/workbench/network/mesh_ssh.py` for the canonical example.
- **Privileged ops:** route through `mackes.admin_session.AdminSession`,
  not raw `pkexec` or `sudo`.
- **CSS:** tokens (palette / type ramp) live in `data/css/tokens.css`.
  Layout classes are in `data/css/carbon-layout.css`. Per-preset
  accents are in `data/css/accents/*.css`.
- **Spacing:** strict 8px grid (4px allowed for tight controls).
- **Commits:** one logical change per commit, focused on **why** in the
  message. See `.claude/CLAUDE.md §0` for the full rulebook.
- **No `--amend` of pushed commits.** Roll forward with a new commit
  instead.

## Release flow

Maintainers ship via tag-push: bump version in four places
(`mackes/__init__.py`, `pyproject.toml`, `setup.py`,
`packaging/fedora/mackes-shell.spec`), add a CHANGELOG entry, push,
tag `vX.Y.Z`, push the tag. The release workflow at
`.github/workflows/release.yml` builds the RPM and publishes the
GitHub Release.

## Code of conduct

By participating you agree to the
[Code of Conduct](./CODE_OF_CONDUCT.md). Be kind; be specific; assume
good faith.

## Security

Found a security issue? Don't open a public issue — follow the
disclosure protocol in [SECURITY.md](./SECURITY.md).

## Questions

Open a [Discussion](https://github.com/matthewmackes/MAP2-RELEASES/discussions)
or an issue with the **Question** template.
