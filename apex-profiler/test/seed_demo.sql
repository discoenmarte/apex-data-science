-- Smoke-test schema: shapes the profiler must handle (PK, FK compuesta, comments, stats)
CREATE TABLE pacientes (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    documento VARCHAR(20) NOT NULL UNIQUE,
    nombre TEXT NOT NULL,
    fecha_nacimiento DATE,
    creado_en TIMESTAMPTZ DEFAULT now()
);
COMMENT ON TABLE pacientes IS 'Maestro de pacientes';
COMMENT ON COLUMN pacientes.documento IS 'Documento de identidad';

CREATE TABLE medicos (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    registro VARCHAR(30) NOT NULL,
    especialidad TEXT,
    UNIQUE (registro)
);

CREATE TABLE citas (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    paciente_id BIGINT NOT NULL REFERENCES pacientes(id),
    medico_id BIGINT REFERENCES medicos(id),
    fecha TIMESTAMPTZ NOT NULL,
    estado TEXT CHECK (estado IN ('agendada','cumplida','cancelada'))
);

-- PK compuesta + FK compuesta (caso difícil)
CREATE TABLE cuentas (
    empresa_id INT NOT NULL,
    cuenta_id INT NOT NULL,
    saldo NUMERIC(14,2),
    PRIMARY KEY (empresa_id, cuenta_id)
);
CREATE TABLE cuentas_detalle (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    empresa_id INT NOT NULL,
    cuenta_id INT NOT NULL,
    concepto TEXT,
    valor NUMERIC(14,2),
    FOREIGN KEY (empresa_id, cuenta_id) REFERENCES cuentas(empresa_id, cuenta_id)
);

INSERT INTO pacientes (documento, nombre, fecha_nacimiento)
SELECT 'DOC' || g, 'Paciente ' || g, DATE '1970-01-01' + (g % 15000)
FROM generate_series(1, 500) g;
INSERT INTO medicos (registro, especialidad)
SELECT 'REG' || g, (ARRAY['general','pediatria','cardiologia'])[1 + g % 3]
FROM generate_series(1, 20) g;
INSERT INTO citas (paciente_id, medico_id, fecha, estado)
SELECT 1 + g % 500, CASE WHEN g % 10 = 0 THEN NULL ELSE 1 + g % 20 END,
       now() - (g || ' hours')::interval,
       (ARRAY['agendada','cumplida','cancelada'])[1 + g % 3]
FROM generate_series(1, 2000) g;
INSERT INTO cuentas SELECT e, c, (e * c * 1000)::numeric FROM generate_series(1,5) e, generate_series(1,10) c;
INSERT INTO cuentas_detalle (empresa_id, cuenta_id, concepto, valor)
SELECT 1 + g % 5, 1 + g % 10, 'concepto ' || g, g * 10 FROM generate_series(1, 300) g;

ANALYZE;
