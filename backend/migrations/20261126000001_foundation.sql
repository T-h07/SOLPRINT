-- SOLPRINT foundation schema (Phase 00).
--
-- Creates the application namespace and the single piece of metadata the
-- foundation genuinely needs: a stable identity for this installation and its
-- start history. Trading tables arrive with the phases that own them.
--
-- Migrations are append-only: never edit this file after it has been applied
-- anywhere; add a new migration instead (see docs/development/SETUP.md).

CREATE SCHEMA IF NOT EXISTS solprint;

-- Exactly one row per database. `singleton` is the primary key and is
-- constrained to TRUE, so a second row cannot exist.
CREATE TABLE solprint.instance (
    singleton        BOOLEAN     PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    instance_id      UUID        NOT NULL DEFAULT gen_random_uuid(),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_started_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    start_count      BIGINT      NOT NULL DEFAULT 1 CHECK (start_count >= 1),
    last_app_version TEXT        NOT NULL CHECK (length(last_app_version) BETWEEN 1 AND 64)
);

COMMENT ON TABLE solprint.instance IS
    'Installation identity and start history. One row; upserted on every backend start.';
