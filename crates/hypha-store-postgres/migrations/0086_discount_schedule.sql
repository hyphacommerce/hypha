-- How far the schedule job has announced discounts' window boundaries
-- (`discount.started` / `.ended`): one row, written with the events it announced. No row
-- until the job's first run, which only notes where it starts.
CREATE TABLE discount_schedule (
    id        boolean     PRIMARY KEY DEFAULT true CHECK (id),
    ran_until timestamptz NOT NULL
);
