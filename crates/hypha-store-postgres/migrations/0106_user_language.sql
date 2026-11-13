-- The language a staff member reads the admin in (a BCP 47 tag). Any tag, matched by the
-- admin against its own catalogs. NULL leaves it to the browser's languages, as for
-- every staff member from before.
ALTER TABLE users ADD COLUMN language text;
