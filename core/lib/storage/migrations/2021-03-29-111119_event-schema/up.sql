CREATE TYPE event_type AS ENUM ('ACCOUNT', 'BLOCK', 'TRANSACTION');

CREATE TABLE events (
    id BIGSERIAL PRIMARY KEY,
    event_type event_type NOT NULL,
    event_data jsonb NOT NULL,
    is_processed BOOLEAN NOT NULL
);

CREATE OR REPLACE FUNCTION notify_event_channel() RETURNS TRIGGER AS $$
BEGIN
    PERFORM (
        SELECT pg_notify('event_channel', NEW.id::text)
    );
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER notify_event_listener
AFTER INSERT ON events
FOR EACH ROW EXECUTE PROCEDURE notify_event_channel();
