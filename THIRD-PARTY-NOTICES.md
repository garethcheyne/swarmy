# Third-party notices

Swarmy is licensed under MIT OR Apache-2.0 (see `LICENSE-MIT` and `LICENSE-APACHE`).
It is built on open-source libraries that keep their own licences. Most are Rust crates
and npm packages under MIT, Apache-2.0, ISC or BSD terms; `cargo tree` and
`web/pnpm-lock.yaml` list every one. The parts below have terms worth calling out.

## UnRAR

Swarmy unpacks RAR archives with the UnRAR source code by Alexander L. Roshal, compiled
in through the `unrar` crate. UnRAR is not open source in the OSI sense: it is freeware
with the condition below, which applies to Swarmy as distributed. It means Swarmy, and
any fork of it, may extract RAR archives but must not use this code to create them.

> UnRAR source code may be used in any software to handle RAR archives without
> limitations free of charge, but cannot be used to develop RAR (WinRAR) compatible
> archiver and to re-create RAR compression algorithm, which is proprietary.
> Distribution of modified UnRAR source code in separate form or as a part of other
> software is permitted, provided that full text of this paragraph, starting from
> "UnRAR source code" words, is included in license, or in documentation if license
> is not available, and in source code comments of resulting package.

All copyrights to RAR and UnRAR are owned by Alexander L. Roshal. UnRAR is distributed
"as is", with no warranty of any kind.

## DB-IP IP to City Lite

The swarm map places peers using the free IP to City Lite database by
[DB-IP](https://db-ip.com), licensed under
[Creative Commons Attribution 4.0](https://creativecommons.org/licenses/by/4.0/).
Swarmy downloads it on first start and keeps it up to date; it is not bundled in the
source. The attribution is shown in the app wherever the data is used.

## librqbit

The BitTorrent engine is [librqbit](https://github.com/ikatson/rqbit) by Igor Katson,
licensed under Apache-2.0.

## World map

The map outline comes from [world-atlas](https://github.com/topojson/world-atlas)
(Copyright 2013-2019 Michael Bostock, ISC licence), built from
[Natural Earth](https://www.naturalearthdata.com) data, which is in the public domain.

## Name and logo

The licences cover the code. They do not grant any right to use the name "Swarmy" or
its logo. If you ship a fork, please give it its own name.
