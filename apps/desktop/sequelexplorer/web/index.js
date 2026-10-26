/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

const isMacosBwebview = navigator.userAgent.includes('bwebview') && navigator.userAgent.includes('Macintosh');
if (isMacosBwebview) {
    document.body.classList.add('is-bwebview-macos');
}

window.addEventListener('contextmenu', (e) => e.preventDefault());

const PAGE_SIZE = 100;
const supportsUnixSocket = !navigator.userAgent.includes('Windows');

const ACTIONS = {
    open: 'openDatabase',
    showData: 'showData',
    showSchema: 'showSchema',
    runQuery: 'runQuery',
    clearQuery: 'clearQuery',
};

function ipcSend(type, data = {}) {
    window.ipc.postMessage(JSON.stringify({ type, ...data }));
}

// Preferences are stored natively by the Rust side, it sends them before restoring the last file
const preferences = {
    values: {},
    get(key) {
        return this.values[key] ?? null;
    },
    set(key, value) {
        this.values[key] = value;
        ipcSend('setPreference', { key, value });
    },
    remove(key) {
        delete this.values[key];
        ipcSend('setPreference', { key, value: null });
    },
};

let nextRequestId = 1;
function ipcRequest(type, data = {}) {
    const requestId = nextRequestId++;
    return new Promise((resolve) => {
        const listener = (event) => {
            const message = JSON.parse(event.data);
            if (message.type === `${type}Response` && message.requestId === requestId) {
                window.ipc.removeEventListener('message', listener);
                resolve(message);
            }
        };
        window.ipc.addEventListener('message', listener);
        window.ipc.postMessage(JSON.stringify({ type, requestId, ...data }));
    });
}

function formatSqlValue(cell, isMysql) {
    if (cell.kind === 'null') return 'NULL';
    if (cell.kind === 'integer' || cell.kind === 'float') return String(cell.value);
    if (cell.kind === 'blob') {
        const bytes = Uint8Array.fromBase64(cell.value);
        const hex = Array.from(bytes)
            .map((byte) => byte.toString(16).padStart(2, '0'))
            .join('');
        return `X'${hex.toUpperCase()}'`;
    }
    // MySQL treats backslashes in string literals as escapes by default
    const value = isMysql ? String(cell.value).replace(/\\/g, '\\\\') : String(cell.value);
    return `'${value.replace(/'/g, "''")}'`;
}

function formatCellValue(cell, isBlob) {
    if (cell.kind === 'null') return 'NULL';
    if (!isBlob && cell.kind !== 'blob') return cell.value;

    try {
        const bytes = Uint8Array.fromBase64(cell.value);
        if (bytes.length === 16) {
            const hex = Array.from(bytes)
                .map((byte) => byte.toString(16).padStart(2, '0'))
                .join('');
            return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
        }
        return Array.from(bytes)
            .map((byte) => byte.toString(16).padStart(2, '0'))
            .join('');
    } catch (_error) {
        return cell.value;
    }
}

PetiteVue.createApp({
    dbPath: '',
    dbFileName: '',
    dbOpened: false,
    connectionKind: '',
    connectionLabel: '',
    connectionTab: 'sqlite',
    connectionError: '',
    browserError: '',
    isConnecting: false,
    isSelectingDatabase: false,
    databases: [],
    currentDatabase: '',
    activeDatabase: '',
    supportsUnixSocket,
    mysql: {
        transport: 'tcp',
        host: '127.0.0.1',
        port: 3306,
        socket: '/tmp/mysql.sock',
        user: 'root',
        password: '',
        tls: false,
        remember: true,
    },
    tables: [],
    currentTable: null,
    activeTab: 'data',
    rowCount: '',
    columns: [],
    rows: [],
    showDataTable: false,
    showDataLoading: false,
    showDataEmpty: false,
    dataEmptyText: 'No rows',
    schemaText: '',
    queryText: '',
    isCustomQuery: false,
    isQueryRunning: false,
    currentOffset: 0,
    currentTotal: 0,
    currentCursor: null,
    isLoading: false,
    viewGeneration: 0,

    async init() {
        window.ipc.addEventListener('message', (event) => {
            const message = JSON.parse(event.data);
            if (message.type === 'preferences') preferences.values = message.values;
            if (message.type === 'openFile') this._openDatabaseByPath(message.path);
            if (message.type === 'restoreLastFile') {
                const lastDbPath = preferences.get('dbPath');
                if (lastDbPath) {
                    this._openDatabaseByPath(lastDbPath);
                } else {
                    this.restoreMysqlConnection();
                }
            }
            if (message.type === 'menuAction') this.performAction(message.action);
        });
        const observer = new IntersectionObserver(
            (entries) => {
                if (entries[0].isIntersecting && this.currentTable && !this.isCustomQuery) {
                    this.loadMoreRows(this.currentTable);
                }
            },
            { rootMargin: '200px' },
        );
        observer.observe(this.$refs.loadSentinel);

        const notifyReady = () => requestAnimationFrame(() => requestAnimationFrame(() => ipcSend('ready')));
        if (document.readyState === 'complete') {
            notifyReady();
        } else {
            window.addEventListener('load', notifyReady, { once: true });
        }
    },

    performAction(id) {
        const action = ACTIONS[id];
        if (action) this[action]();
    },

    showData() {
        if (this.currentTable !== null) this.activeTab = 'data';
    },

    showSchema() {
        if (this.currentTable !== null) this.activeTab = 'schema';
    },

    async openDatabase() {
        this.connectionError = '';
        this.$refs.connectionDialog.showModal();
    },

    closeConnectionDialog() {
        if (this.$refs.connectionDialog.open) this.$refs.connectionDialog.close();
    },

    async openSqliteDatabase() {
        const { path } = await ipcRequest('openFileDialog');
        if (!path) return;
        await this._openDatabaseByPath(path);
    },

    async _openDatabaseByPath(path) {
        const { ok, error } = await ipcRequest('openDatabase', { path });
        if (!ok) {
            if (preferences.get('dbPath') === path) {
                preferences.remove('dbPath');
            }
            await ipcRequest('showMessage', { title: 'Failed to open database', description: error });
            return;
        }
        this.dbPath = path;
        this.dbFileName = path.replace(/.*[\\/]/, '');
        this.connectionKind = 'sqlite';
        this.connectionLabel = this.dbFileName;
        this.currentDatabase = this.dbFileName;
        this.activeDatabase = '';
        this.databases = [];
        preferences.set('dbPath', path);
        this.dbOpened = true;
        this.closeConnectionDialog();
        this.resetBrowser();
        document.title = `Sequel Explorer - ${this.dbFileName}`;
        await this.loadTables();
        const lastTable = preferences.get('tableName');
        if (lastTable && this.tables.includes(lastTable)) {
            await this.selectTable(lastTable);
        }
    },

    async restoreMysqlConnection() {
        const savedConnection = preferences.get('mysqlConnection');
        if (!savedConnection) return;
        try {
            const connection = JSON.parse(savedConnection);
            if (!connection || !connection.transport || !connection.user) return;
            this.mysql = { ...this.mysql, ...connection, password: '', remember: true };
            this.connectionTab = 'mysql';
            if (!(await this._openMysql(true))) this.$refs.connectionDialog.showModal();
        } catch (_error) {
            preferences.remove('mysqlConnection');
        }
    },

    async openMysql() {
        return this._openMysql(false);
    },

    async _openMysql(useSavedPassword) {
        if (this.isConnecting) return;
        this.isConnecting = true;
        this.connectionError = '';
        let response;
        try {
            let previousConnection = null;
            try {
                const savedConnection = JSON.parse(preferences.get('mysqlConnection'));
                if (savedConnection?.transport && savedConnection?.user) previousConnection = savedConnection;
            } catch (_error) {
                preferences.remove('mysqlConnection');
            }
            response = await ipcRequest('openMysql', {
                ...this.mysql,
                password: useSavedPassword ? null : this.mysql.password,
                previousConnection,
            });
        } finally {
            this.isConnecting = false;
        }
        if (!response.ok) {
            this.connectionError = response.error || 'Failed to connect';
            return false;
        }

        this.mysql.password = '';
        if (this.mysql.remember && response.credentialSaved) {
            const { transport, host, port, socket, user, tls } = this.mysql;
            preferences.set('mysqlConnection', JSON.stringify({ transport, host, port, socket, user, tls }));
        } else {
            preferences.remove('mysqlConnection');
        }
        if (response.credentialError) {
            await ipcRequest('showMessage', {
                title: 'Connected, but credentials could not be saved securely',
                description: response.credentialError,
                warning: true,
            });
        }

        preferences.remove('dbPath');
        this.dbPath = '';
        this.dbFileName = this.mysql.transport === 'tcp' ? this.mysql.host : this.mysql.socket;
        this.connectionKind = 'mysql';
        this.connectionLabel =
            this.mysql.transport === 'tcp'
                ? `${this.mysql.user}@${this.mysql.host}:${this.mysql.port}`
                : `${this.mysql.user}@${this.mysql.socket}`;
        this.dbOpened = true;
        this.currentDatabase = '';
        this.activeDatabase = '';
        this.closeConnectionDialog();
        this.resetBrowser();
        document.title = `Sequel Explorer - ${this.connectionLabel}`;
        await this.loadDatabases();

        const lastDatabase = preferences.get('mysqlDatabase');
        const database = this.databases.includes(lastDatabase) ? lastDatabase : this.databases[0];
        if (database) await this.selectDatabase(database);
        return true;
    },

    async loadDatabases() {
        const generation = this.viewGeneration;
        try {
            const response = await fetch('/api/databases');
            const data = await response.json();
            if (generation !== this.viewGeneration) return;
            if (data.error) {
                this.browserError = data.error;
                this.databases = [];
                return;
            }
            this.browserError = '';
            this.databases = data;
        } catch (error) {
            if (generation !== this.viewGeneration) return;
            this.browserError = `Failed to load databases: ${error.message}`;
            this.databases = [];
        }
    },

    async selectDatabase(database) {
        if (!database || this.isSelectingDatabase) return;
        this.isSelectingDatabase = true;
        let response;
        try {
            response = await ipcRequest('selectMysqlDatabase', { database });
        } finally {
            this.isSelectingDatabase = false;
        }
        if (!response.ok) {
            this.currentDatabase = this.activeDatabase;
            await ipcRequest('showMessage', { title: 'Failed to open database', description: response.error });
            return;
        }
        this.currentDatabase = database;
        this.activeDatabase = database;
        this.dbFileName = database;
        preferences.set('mysqlDatabase', database);
        this.resetBrowser();
        document.title = `Sequel Explorer - ${this.connectionLabel} - ${database}`;
        await this.loadTables();
        const lastTable = preferences.get('tableName');
        if (lastTable && this.tables.includes(lastTable)) await this.selectTable(lastTable);
    },

    resetBrowser() {
        this.viewGeneration += 1;
        this.isLoading = false;
        this.isQueryRunning = false;
        this.tables = [];
        this.currentTable = null;
        this.currentOffset = 0;
        this.currentTotal = 0;
        this.currentCursor = null;
        this.columns = [];
        this.rows = [];
        this.rowCount = '';
        this.schemaText = '';
        this.queryText = '';
        this.isCustomQuery = false;
        this.activeTab = 'data';
        this.showDataTable = false;
        this.showDataLoading = false;
        this.showDataEmpty = false;
        this.browserError = '';
    },

    async loadTables() {
        const generation = this.viewGeneration;
        try {
            const response = await fetch('/api/tables');
            const data = await response.json();
            if (generation !== this.viewGeneration) return;
            if (data.error) {
                this.browserError = data.error;
                this.tables = [];
                return;
            }
            this.browserError = '';
            this.tables = data;
        } catch (error) {
            if (generation !== this.viewGeneration) return;
            this.browserError = `Failed to load tables: ${error.message}`;
            this.tables = [];
        }
    },

    async selectTable(name) {
        if (name === this.currentTable) return;
        this.currentTable = name;
        this.isCustomQuery = false;
        this.queryText = '';
        preferences.set('tableName', name);
        await this.openTableView(name);
    },

    async openTableView(name) {
        const generation = ++this.viewGeneration;
        document.title = `Sequel Explorer - ${this.dbFileName} - ${name}`;

        this.isLoading = false;
        this.currentOffset = 0;
        this.currentTotal = 0;
        this.currentCursor = null;
        this.isCustomQuery = false;
        this.columns = [];
        this.rows = [];
        this.rowCount = '';
        this.showDataEmpty = false;
        this.showDataLoading = true;
        this.showDataTable = false;
        this.activeTab = 'data';

        await this.loadMoreRows(name, generation);
        if (generation !== this.viewGeneration || name !== this.currentTable) return;

        try {
            const response = await fetch(`/api/table/${encodeURIComponent(name)}/schema`);
            const data = await response.json();
            if (generation !== this.viewGeneration || name !== this.currentTable) return;
            this.schemaText = data.error ? 'Error: ' + data.error : data.sql || '';
        } catch (error) {
            if (generation !== this.viewGeneration || name !== this.currentTable) return;
            this.schemaText = 'Error loading schema: ' + error.message;
        }
    },

    async loadMoreRows(tableName, generation = this.viewGeneration) {
        if (generation !== this.viewGeneration || tableName !== this.currentTable) return;
        if (this.isLoading) return;
        if (this.currentOffset > 0 && this.currentOffset >= this.currentTotal) return;

        const offset = this.currentOffset;
        this.isLoading = true;
        this.showDataLoading = true;

        try {
            const params = new URLSearchParams({ offset, limit: PAGE_SIZE });
            if (this.currentCursor) params.set('cursor', this.currentCursor);
            const url = `/api/table/${encodeURIComponent(tableName)}/data?${params}`;
            const response = await fetch(url);
            const data = await response.json();
            if (generation !== this.viewGeneration || tableName !== this.currentTable) return;

            if (data.error) {
                this.dataEmptyText = 'Error: ' + data.error;
                this.showDataEmpty = true;
                return;
            }

            if (offset === 0) {
                this.currentTotal = data.total;
                this.rowCount = `${data.total.toLocaleString()} rows`;
                this.columns = data.columns;
                this.showDataTable = true;
                if (data.rows.length === 0) {
                    this.dataEmptyText = 'No rows';
                    this.showDataEmpty = true;
                    return;
                }
            }

            this.appendRows(data.rows);
            this.currentOffset = offset + data.rows.length;
            this.currentCursor = data.next_cursor;
        } catch (error) {
            if (generation !== this.viewGeneration || tableName !== this.currentTable) return;
            this.dataEmptyText = `Failed to load rows: ${error.message}`;
            this.showDataEmpty = true;
        } finally {
            if (generation === this.viewGeneration && tableName === this.currentTable) {
                this.showDataLoading = false;
                this.isLoading = false;
            }
        }
    },

    appendRows(rows) {
        this.rows = this.rows.concat(rows);
    },

    formatSqlValue(cell) {
        return formatSqlValue(cell, this.connectionKind === 'mysql');
    },

    quoteIdentifier(identifier) {
        if (this.connectionKind === 'mysql') return '`' + identifier.replace(/`/g, '``') + '`';
        return '"' + identifier.replace(/"/g, '""') + '"';
    },

    async navigateToForeignKey(table, column, cell) {
        this.queryText = `SELECT * FROM ${this.quoteIdentifier(table)} WHERE ${this.quoteIdentifier(column)} = ${this.formatSqlValue(cell)}`;
        await this.runQuery();
    },

    async runQuery() {
        const sql = this.queryText.trim();
        // Guard against re-entry, the same query can be triggered by the input, the button and a shortcut
        if (!sql || this.isQueryRunning) return;

        const generation = ++this.viewGeneration;
        this.isLoading = false;
        this.isQueryRunning = true;
        this.isCustomQuery = true;
        this.activeTab = 'data';
        this.currentOffset = 0;
        this.currentTotal = 0;
        this.currentCursor = null;
        this.columns = [];
        this.rows = [];
        this.rowCount = '';
        this.showDataEmpty = false;
        this.showDataLoading = true;
        this.showDataTable = false;

        try {
            const response = await fetch('/api/query', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ sql }),
            });
            const data = await response.json();
            if (generation !== this.viewGeneration) return;

            if (data.error) {
                this.dataEmptyText = 'Error: ' + data.error;
                this.showDataEmpty = true;
                return;
            }

            this.rowCount = data.truncated
                ? `${data.rows.length.toLocaleString()}+ rows`
                : `${data.rows.length.toLocaleString()} rows`;
            this.columns = data.columns;
            this.showDataTable = true;

            if (data.rows.length === 0) {
                this.dataEmptyText = 'No rows';
                this.showDataEmpty = true;
                return;
            }

            this.appendRows(data.rows);
        } catch (error) {
            if (generation !== this.viewGeneration) return;
            this.dataEmptyText = `Query failed: ${error.message}`;
            this.showDataEmpty = true;
        } finally {
            if (generation === this.viewGeneration) {
                this.isQueryRunning = false;
                this.showDataLoading = false;
            }
        }
    },

    clearQuery() {
        this.queryText = '';
        this.isCustomQuery = false;
        if (this.currentTable) this.openTableView(this.currentTable);
    },

    formatCellValue(cell, colIdx) {
        const column = this.columns[colIdx];
        return formatCellValue(cell, column.is_blob);
    },
}).mount('#app');
