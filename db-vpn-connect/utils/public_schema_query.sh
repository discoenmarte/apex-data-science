#!/bin/bash -i

case "$1" in
  blueprint)
    PGPASSWORD="$DB_PASSWORD" psql -h localhost -U $DB_USER -d $DB_NAME -f utils/public_schema_blueprint.sql -t -A > data/public_schema_blueprint.json
    ;;
  table_sizes)
    PGPASSWORD="$DB_PASSWORD" psql -h localhost -U $DB_USER -d $DB_NAME -f utils/public_schema_table_sizes.sql --csv > data/public_schema_table_sizes.csv
    ;;
  total_size)
    PGPASSWORD="$DB_PASSWORD" psql -h localhost -U $DB_USER -d $DB_NAME -f utils/public_schema_total_size.sql --csv > data/public_schema_total_size.csv
    ;;
  *)
    echo "Usage: $0 {blueprint|table_sizes|total_size}"
    exit 1
    ;;
esac