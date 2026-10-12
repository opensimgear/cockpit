# SimHub integration profiles

Add versioned, ready-to-use profiles for the agreed serial protocol and telemetry mappings. Initial support is Windows-specific. Profiles and their output freshness/session behavior require real validation. Automatic profile generation is a proposal, not a committed feature. Multiple gateways require distinct serial endpoints/profiles bound to stable gateway identity and destination module identities; chain-local node IDs alone are insufficient. Update output routing after a module moves gateways, without replaying stale commands. No profiles are supplied yet.
