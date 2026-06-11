# Changelog

## [0.4.3](https://github.com/psedge/fijit/compare/v0.4.2...v0.4.3) (2026-06-11)


### ⚠ BREAKING CHANGES

* drop global config; obscura via --obscura or $PATH, scrapers self-contained

### Features

* add sort/compute/follow steps, new alert triggers and in-file state ([0884e3a](https://github.com/psedge/fijit/commit/0884e3a5474fd781c2deef01aa3c1b16b65ab8cb))
* drop global config; obscura via --obscura or $PATH, scrapers self-contained ([0865649](https://github.com/psedge/fijit/commit/0865649df173c2ab435e45ccb1d6db6553d3f8b8))

## [0.4.2](https://github.com/psedge/fijit/compare/v0.4.1...v0.4.2) (2026-06-10)


### Features

* capture arbitrary element attributes via query_all attrs ([8700a31](https://github.com/psedge/fijit/commit/8700a31770cf16eae110cbba9df396f4e6f05a4a))


### Bug Fixes

* key change-alert state by explicit id, not step position ([df2cfc4](https://github.com/psedge/fijit/commit/df2cfc4a0b0707a30047f94a4594aea4606dc086))

## [0.4.1](https://github.com/psedge/fijit/compare/v0.4.0...v0.4.1) (2026-06-10)


### Features

* document filter/find operators in init-config template ([6d29979](https://github.com/psedge/fijit/commit/6d29979cc1a3e7ebf1798b7719c3b7ac59fc07bf))

## [0.4.0](https://github.com/psedge/fijit/compare/v0.3.0...v0.4.0) (2026-06-10)


### Features

* log scraper output to per-scraper file in fijit schedule ([c32ba4d](https://github.com/psedge/fijit/commit/c32ba4d9c4729df9f723bf1cbe9036814892869d))
* on_error block sends scraper errors to Slack ([c76db8b](https://github.com/psedge/fijit/commit/c76db8bb0d54872198e12859081b90ce90086f5c))


### Bug Fixes

* log to /var/log/fijit/&lt;name&gt;.log instead of ~/.local ([a326e33](https://github.com/psedge/fijit/commit/a326e33ddd9fd4e3682ea72e14242e5a4c9b6689))
* verify /var/log/fijit is writable in fijit schedule with helpful error ([8f46900](https://github.com/psedge/fijit/commit/8f46900ab77e48f5d3785eec0cef5cacc3390c88))

## [0.3.0](https://github.com/psedge/fijit/compare/v0.2.0...v0.3.0) (2026-06-10)


### Features

* update init-config template with action and trigger reference ([49ac9d5](https://github.com/psedge/fijit/commit/49ac9d5c755dc16cacf8bd7ad24221791b4c09d9))

## [0.2.0](https://github.com/psedge/fijit/compare/v0.1.0...v0.2.0) (2026-06-09)


### Features

* fijit initial release ([e4dc110](https://github.com/psedge/fijit/commit/e4dc1105e39749cdc8a439314fded4a08812e641))
