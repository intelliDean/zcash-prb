-- Synthetic Protocol Test Fixture: Mock YWallet Schema & Table State
-- Purpose: Protocol verification of schema fields and offline SQLite table layout.
-- Note: This is a synthetic mock fixture designed for testing, not an export from a live mainnet wallet.
-- For cryptographically derived valid viewing keys, see `docs/fixtures/public_watch_only_fixture.json`.

PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;

CREATE TABLE IF NOT EXISTS accounts (
    id_account INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    seed TEXT,
    aindex INTEGER NOT NULL,
    sk TEXT,
    ivk TEXT NOT NULL UNIQUE,
    address TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS transactions (
    id_tx INTEGER PRIMARY KEY,
    account INTEGER NOT NULL,
    txid BLOB NOT NULL,
    height INTEGER NOT NULL,
    timestamp INTEGER NOT NULL,
    value INTEGER NOT NULL,
    address TEXT,
    memo TEXT,
    tx_index INTEGER,
    messages BLOB NULL,
    CONSTRAINT tx_account UNIQUE (height, tx_index, account)
);

CREATE TABLE IF NOT EXISTS received_notes (
    id_note INTEGER PRIMARY KEY,
    account INTEGER NOT NULL,
    position INTEGER NOT NULL,
    tx INTEGER NOT NULL,
    height INTEGER NOT NULL,
    output_index INTEGER NOT NULL,
    diversifier BLOB NOT NULL,
    value INTEGER NOT NULL,
    rcm BLOB NOT NULL,
    nf BLOB NOT NULL UNIQUE,
    rho BLOB,
    orchard BOOL NOT NULL DEFAULT false,
    spent INTEGER,
    excluded BOOL,
    CONSTRAINT tx_output UNIQUE (tx, orchard, output_index)
);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY,
    account INTEGER NOT NULL,
    sender TEXT,
    recipient TEXT NOT NULL,
    subject TEXT NOT NULL,
    body TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    height INTEGER NOT NULL,
    read BOOL NOT NULL,
    id_tx INTEGER,
    incoming BOOL NOT NULL DEFAULT true,
    vout INTEGER NOT NULL DEFAULT(0)
);

-- Accounts
INSERT INTO accounts (id_account, name, seed, aindex, sk, ivk, address)
VALUES (
    1,
    'Parity-Watch-Only-Grant-Review',
    NULL,
    0,
    NULL,
    'zxviews1q0ej7k8e24k5r24x8j82s7y6x7y2e2w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980',
    'u1w24w657q22u8e88849646y28kwh9784y48820w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q298'
);

-- Transactions
INSERT INTO transactions (id_tx, account, txid, height, timestamp, value, address, memo, tx_index, messages)
VALUES (
    1,
    1,
    X'8b6ceacbb27d4cb54bccb62f6632da385189361f895fc8b924ab143052b0b48b',
    2500000,
    1702500000,
    25000000,
    'u1w24w657q22u8e88849646y28kwh9784y48820w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q298',
    'Grant Evaluation Parity Receipt - 2026-Q4',
    0,
    NULL
);

INSERT INTO transactions (id_tx, account, txid, height, timestamp, value, address, memo, tx_index, messages)
VALUES (
    2,
    1,
    X'6c41a5f6a7fff02dafa92c437862b80a9d77eaee6bbbce70f353c621585b8e7f',
    2500001,
    1702500075,
    100000000,
    'u1w24w657q22u8e88849646y28kwh9784y48820w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q298',
    'Grant Evaluation Parity Receipt - 2026-Q4',
    0,
    NULL
);

-- Received Notes (Pool 0: Sapling, Pool 1: Orchard)
INSERT INTO received_notes (id_note, account, position, tx, height, output_index, diversifier, value, rcm, nf, rho, orchard, spent, excluded)
VALUES (
    1,
    1,
    0,
    1,
    2500000,
    0,
    X'0102030405060708090a0b',
    25000000,
    X'a1a2a3a4a5a6a7a8a9b1b2b3b4b5b6b7b8b9c1c2c3c4c5c6c7c8c9d1d2d3d4d5',
    X'f1f2f3f4f5f6f7f8f9e1e2e3e4e5e6e7e8e9d1d2d3d4d5d6d7d8d9c1c2c3c4c5',
    NULL,
    0,
    NULL,
    0
);

INSERT INTO received_notes (id_note, account, position, tx, height, output_index, diversifier, value, rcm, nf, rho, orchard, spent, excluded)
VALUES (
    2,
    1,
    1,
    2,
    2500001,
    0,
    X'0102030405060708090a0b',
    100000000,
    X'b1b2b3b4b5b6b7b8b9c1c2c3c4c5c6c7c8c9d1d2d3d4d5d6d7d8d9e1e2e3e4e5',
    X'e1e2e3e4e5e6e7e8e9d1d2d3d4d5d6d7d8d9c1c2c3c4c5c6c7c8c9b1b2b3b4b5',
    NULL,
    1,
    NULL,
    0
);

-- Messages (Decrypted Memo Plaintexts)
INSERT INTO messages (id, account, sender, recipient, subject, body, timestamp, height, read, id_tx, incoming, vout)
VALUES (
    1,
    1,
    NULL,
    'u1w24w657q22u8e88849646y28kwh9784y48820w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q298',
    '',
    'Grant Evaluation Parity Receipt - 2026-Q4',
    1702500000,
    2500000,
    1,
    1,
    1,
    0
);

INSERT INTO messages (id, account, sender, recipient, subject, body, timestamp, height, read, id_tx, incoming, vout)
VALUES (
    2,
    1,
    NULL,
    'u1w24w657q22u8e88849646y28kwh9784y48820w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q2980w8s8j48q298',
    '',
    'Grant Evaluation Parity Receipt - 2026-Q4',
    1702500075,
    2500001,
    1,
    2,
    1,
    0
);

COMMIT;
