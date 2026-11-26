#!/bin/sh
# Runs once, on first initialisation of an empty data volume.
#
# Creates the least-privilege application role and its database:
#   * POSTGRES_USER (superuser) is for administration only.
#   * solprint_app owns the `solprint` database (so it can run migrations)
#     but is not a superuser and cannot create roles or databases.
set -eu

: "${SOLPRINT_DB_PASSWORD:?SOLPRINT_DB_PASSWORD must be set}"

psql -v ON_ERROR_STOP=1 \
     --username "$POSTGRES_USER" --dbname postgres \
     --set=app_password="$SOLPRINT_DB_PASSWORD" <<'SQL'
CREATE ROLE solprint_app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION
    PASSWORD :'app_password';
CREATE DATABASE solprint OWNER solprint_app;
REVOKE ALL ON DATABASE solprint FROM PUBLIC;
GRANT CONNECT, TEMPORARY ON DATABASE solprint TO solprint_app;
\connect solprint
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
ALTER SCHEMA public OWNER TO solprint_app;
SQL
