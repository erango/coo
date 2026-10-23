-- Delivery facts from a message's headers (identity::Origin as JSON):
-- Reply-To, Delivered-To and SPF/DKIM/DMARC results, for the sender card.
ALTER TABLE bodies ADD COLUMN origin_json TEXT;
