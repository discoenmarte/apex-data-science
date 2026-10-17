import json
from collections import defaultdict


class Column:
    def __init__(self, name, data_type, is_nullable=True, default_value=None, comment=None):
        self.name = name
        self.data_type = data_type
        self.is_nullable = is_nullable
        self.default_value = default_value
        self.comment = comment

    def __repr__(self):
        nullable = "NULL" if self.is_nullable else "NOT NULL"
        default = f" DEFAULT {self.default_value}" if self.default_value is not None else ""
        return f"<Column {self.name} {self.data_type} {nullable}{default}>"

    def to_dict(self):
        return {
            "name": self.name,
            "data_type": self.data_type,
            "is_nullable": self.is_nullable,
            "default_value": self.default_value,
            "comment": self.comment,
        }


class Table:
    def __init__(self, schema, name, columns, comment=None, constraints=None):
        self.schema = schema
        self.name = name
        self.columns = [
            col if isinstance(col, Column) else Column(
                name=col.get("name"),
                data_type=col.get("data_type"),
                is_nullable=col.get("is_nullable", True),
                default_value=col.get("default_value"),
                comment=col.get("comment"),
            )
            for col in columns
        ]
        self.comment = comment
        self.constraints = constraints or []  # list of dicts from JSON (foreign keys, etc.)

    def column_names(self):
        return [col.name for col in self.columns]

    def column_count(self):
        return len(self.columns)

    def find_column(self, substring):
        substring = substring.lower()
        return [col for col in self.columns if substring in col.name.lower()]

    def find_in_comment(self, substring):
        return bool(self.comment and substring.lower() in self.comment.lower())

    def describe(self):
        lines = [f"Table: {self.schema}.{self.name}"]
        if self.comment:
            lines.append(f"Comment: {self.comment}")
        lines.append(f"Columns ({len(self.columns)}):")
        for col in self.columns:
            lines.append(
                f"  - {col.name} {col.data_type}"
                f"{' NOT NULL' if not col.is_nullable else ''}"
                f"{' DEFAULT ' + str(col.default_value) if col.default_value else ''}"
                f"{' -- ' + col.comment if col.comment else ''}"
            )
        if self.constraints:
            lines.append("Constraints:")
            for c in self.constraints:
                lines.append(f"  - {c.get('constraint_name')}: {c.get('definition')}")
        return "\n".join(lines)

    def create_sql(self):
        cols = []
        for col in self.columns:
            line = f'"{col.name}" {col.data_type}'
            if not col.is_nullable:
                line += " NOT NULL"
            if col.default_value is not None:
                line += f" DEFAULT {col.default_value}"
            cols.append(line)
        cols_sql = ",\n  ".join(cols)
        return f'CREATE TABLE "{self.schema}"."{self.name}" (\n  {cols_sql}\n);'

    def drop_sql(self):
        return f'DROP TABLE IF EXISTS "{self.schema}"."{self.name}" CASCADE;'

    def __repr__(self):
        return f"<Table {self.schema}.{self.name} ({self.column_count()} columns)>"


class Database:
    def __init__(self, tables):
        self.tables = tables  # List[Table]
        self.schemas = defaultdict(list)
        for table in tables:
            self.schemas[table.schema].append(table)

    @classmethod
    def from_json(cls, path_or_obj):
        if isinstance(path_or_obj, str):
            with open(path_or_obj, "r", encoding="utf-8") as f:
                data = json.load(f)
        else:
            data = path_or_obj
        tables = [
            Table(
                schema=item["schema"],
                name=item["table"],
                columns=item.get("columns", []),
                comment=item.get("comment"),
                constraints=item.get("constraints"),
            )
            for item in data
        ]
        return cls(tables)

    # ---------------------------
    # Basic exploration
    # ---------------------------
    def schema_names(self):
        return list(self.schemas.keys())

    def table_names(self, schema=None):
        if schema:
            return [t.name for t in self.schemas.get(schema, [])]
        return [t.name for t in self.tables]

    def table_count(self, schema=None):
        if schema:
            return len(self.schemas.get(schema, []))
        return len(self.tables)

    def column_count(self, schema=None):
        if schema:
            return sum(t.column_count() for t in self.schemas.get(schema, []))
        return sum(t.column_count() for t in self.tables)

    def summary(self):
        lines = [
            f"Database Summary:",
            f"  Schemas: {len(self.schemas)}",
            f"  Tables: {len(self.tables)}",
            f"  Columns (total): {self.column_count()}",
        ]
        for schema, tables in self.schemas.items():
            lines.append(f"  - {schema}: {len(tables)} tables")
        return "\n".join(lines)

    # ---------------------------
    # Lookup & Search
    # ---------------------------
    def get_table(self, schema, name):
        for t in self.schemas.get(schema, []):
            if t.name == name:
                return t
        return None

    def find_tables(self, substring):
        substring = substring.lower()
        return [t for t in self.tables if substring in t.name.lower()]

    def find_columns(self, substring):
        substring = substring.lower()
        results = []
        for t in self.tables:
            for col in t.columns:
                if substring in col.name.lower():
                    results.append((t, col))
        return results

    def find_in_comments(self, substring):
        substring = substring.lower()
        results = []
        for t in self.tables:
            if t.comment and substring in t.comment.lower():
                results.append(t)
            for col in t.columns:
                if col.comment and substring in col.comment.lower():
                    results.append((t, col))
        return results

    # ---------------------------
    # Relationship analysis
    # ---------------------------
    def find_relationships(self, table_name=None):
        """
        Extracts all relationships (foreign keys) from the JSON constraints.
        Returns a list of dicts:
          {
            "table": "public.orders",
            "references": "public.customers",
            "fk_column": "customer_id",
            "pk_column": "id",
            "constraint_name": "...",
          }
        If `table_name` is provided, filters by that table (source or target).
        """
        relationships = []

        for t in self.tables:
            for c in t.constraints or []:
                if c.get("constraint_type") != "f":
                    continue  # only foreign keys
                defn = c.get("definition", "")
                rel = {
                    "constraint_name": c.get("constraint_name"),
                    "table": f"{t.schema}.{t.name}",
                    "definition": defn,
                    "references": None,
                    "fk_column": None,
                    "pk_column": None,
                }

                # Attempt to parse the constraint definition text
                # Example: FOREIGN KEY (paciente_id) REFERENCES public.pacientes(id)
                import re
                m = re.search(
                    r"FOREIGN KEY\s*\((.*?)\)\s*REFERENCES\s+([\w\.]+)\s*\((.*?)\)",
                    defn, re.IGNORECASE
                )
                if m:
                    rel["fk_column"] = m.group(1).strip()
                    rel["references"] = m.group(2).strip()
                    rel["pk_column"] = m.group(3).strip()

                relationships.append(rel)

        if table_name:
            table_name = table_name.lower()
            relationships = [
                r for r in relationships
                if table_name in r["table"].lower() or
                   (r["references"] and table_name in r["references"].lower())
            ]
        return relationships

    # ---------------------------
    # Dunder & convenience
    # ---------------------------
    def __contains__(self, item):
        if isinstance(item, tuple) and len(item) == 2:
            schema, name = item
            return any(t.name == name for t in self.schemas.get(schema, []))
        elif isinstance(item, str):
            return any(t.name == item for t in self.tables)
        return False

    def __repr__(self):
        return f"<Database: {len(self.tables)} tables across {len(self.schemas)} schemas>"
