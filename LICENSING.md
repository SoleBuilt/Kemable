# Licensing

Kemable is open source and is also sold under a commercial license. This file
says which license covers what, and where to ask.

## The code: AGPL-3.0-only

Everything in this repository is licensed under the GNU Affero General Public
License, version 3 only ([LICENSE](LICENSE)), except the paths listed under
[Other licenses](#other-licenses). In short, you may use, study, change and share
Kemable. If you share it, changed or not, or let others use a changed version
over a network, you must offer them its source under the same license.

Copyright (C) 2026 Huỳnh Trung Đức.

## Commercial license

A commercial license is for anyone who wants to use Kemable without the AGPL's
conditions, for example to ship it inside a product whose source stays closed.
Premium features will come under it too.

The editions, their features and their prices are not published yet. To ask
about a commercial license, or to hear when it is offered, write to
[huynhtrungduc.growth@gmail.com](mailto:huynhtrungduc.growth@gmail.com).

## Contributions

Contributors sign the [Contributor License Agreement](CLA.md) on their first
pull request. It lets the project offer their contributions under both the AGPL
and the commercial license. Contributors keep their copyright.

## The name and the logo

The names Kemable and SoleBuilt and the Kemable icon and logo are not covered by
any of the licenses above. [TRADEMARKS.md](TRADEMARKS.md) says how you may use
them.

## Other licenses

| Path | License |
|---|---|
| `ui/windows/src/lib/ui/Icon.svelte`, the icons' path data | ISC, and MIT for the icons from Feather: the [Lucide license](ui/windows/src/lib/ui/LUCIDE-LICENSE) |

## Third-party software

The libraries and fonts inside the app keep their own licenses, quoted in full in
[ui/windows/THIRD-PARTY-NOTICES.md](ui/windows/THIRD-PARTY-NOTICES.md), which
ships with the app (Settings › About › Third-party notices). A dependency must
have a license that allows both the AGPL and the commercial build:
[deny.toml](deny.toml) lists them, and every pull request checks it.
