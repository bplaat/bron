/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Polyfills
if (!Uint8Array.fromBase64) {
    Uint8Array.fromBase64 = (string) => Uint8Array.from(atob(string), (char) => char.charCodeAt(0));
}

if (!Object.fromEntries) {
    Object.fromEntries = (entries) => {
        const object = {};
        for (const [key, value] of entries) object[key] = value;
        return object;
    };
}

if (!navigator.clipboard) {
    Object.defineProperty(navigator, 'clipboard', {
        value: {
            writeText(text) {
                const textarea = document.createElement('textarea');
                textarea.value = text;
                textarea.style.position = 'fixed';
                textarea.style.opacity = '0';
                document.body.appendChild(textarea);
                textarea.select();
                const copied = document.execCommand('copy');
                textarea.remove();
                return copied ? Promise.resolve() : Promise.reject(new Error('Copy to the clipboard failed'));
            },
        },
    });
}

// App
const isMacosBwebview = navigator.userAgent.includes('bwebview') && navigator.userAgent.includes('Macintosh');
if (isMacosBwebview) {
    document.body.classList.add('is-bwebview-macos');
}

const PAGE_SIZE = 100;
const supportsUnixSocket = !navigator.userAgent.includes('Windows');

const ACTIONS = {
    open: 'openDatabase',
    newTable: 'openCreateTableDialog',
    newDatabase: 'openCreateDatabaseDialog',
    dropDatabase: 'dropCurrentDatabase',
    manageUsers: 'openUsersDialog',
    duplicateTable: 'openDuplicateTableDialog',
    truncateTable: 'truncateTable',
    dropTable: 'dropTable',
    addRow: 'openAddRowDialog',
    editRow: 'openEditRowDialog',
    duplicateRows: 'duplicateSelectedRows',
    deleteRows: 'deleteSelectedRows',
    copyRows: 'copySelectedRows',
    copyRowsAsInsert: 'copySelectedRowsAsInsert',
    importSql: 'importSql',
    exportSql: 'exportSql',
    showData: 'showData',
    showSchema: 'showSchema',
    showQuery: 'showQuery',
    runQuery: 'runQuery',
    clearQuery: 'clearQuery',
    refresh: 'refresh',
};

const FILTER_OPERATORS = [
    { value: 'contains', label: 'contains' },
    { value: 'notContains', label: 'does not contain' },
    { value: '=', label: '=' },
    { value: '!=', label: '≠' },
    { value: '<', label: '<' },
    { value: '<=', label: '≤' },
    { value: '>', label: '>' },
    { value: '>=', label: '≥' },
    { value: 'startsWith', label: 'begins with' },
    { value: 'endsWith', label: 'ends with' },
    { value: 'isNull', label: 'is NULL' },
    { value: 'isNotNull', label: 'is not NULL' },
];
const FOREIGN_KEY_ACTIONS = ['NO ACTION', 'RESTRICT', 'CASCADE', 'SET NULL', 'SET DEFAULT'];

function ipcSend(type, data = {}) {
    window.ipc.postMessage(JSON.stringify({ type, ...data }));
}

// Preferences are stored natively by the Rust side, it sends them before restoring the last file
const preferences = {
    values: {},
    get(key) {
        const value = this.values[key];
        return value === undefined ? null : value;
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

function emptySchemaColumn() {
    return {
        name: '',
        type: 'TEXT',
        nullable: true,
        defaultSql: '',
        primaryKey: false,
        primaryKeyPosition: 0,
        autoIncrement: false,
        generated: false,
        characterSet: null,
        collation: null,
        comment: '',
        extra: '',
    };
}

function emptySchemaIndex() {
    return { name: '', columns: [], unique: false, primary: false, readOnly: false };
}

function emptyForeignKey() {
    return {
        name: '',
        column: '',
        referencedTable: '',
        referencedColumn: '',
        onUpdate: 'NO ACTION',
        onDelete: 'NO ACTION',
    };
}

function emptyFilter() {
    return { column: '', operator: 'contains', value: '' };
}

let nextRequestId = 1;
let modalFocus = null;
let backdropPressed = false;
let rawQueryGeneration = 0;
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

function showMessage(title, description, level = 'error') {
    return ipcRequest('showMessage', { title, description, level });
}

async function confirmAction(title, description, confirm) {
    const { confirmed } = await ipcRequest('showMessage', { title, description, level: 'warning', confirm });
    return confirmed;
}

// Calls a JSON API route over IPC, database errors are returned in the error field of the result
async function api(path, method = 'GET', body = null) {
    const { data } = await ipcRequest('apiRequest', { method, path, body });
    return data;
}

function tableUrl(table, path) {
    return `/api/table/${encodeURIComponent(table)}/${path}`;
}

// Blobs travel as base64 over IPC and are shown and edited as UUIDs or hex bytes
const UUID_PATTERN = /^[0-9a-f]{8}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{12}$/i;

function hexFromBytes(bytes) {
    return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

function base64FromBytes(bytes) {
    return btoa(Array.from(bytes, (byte) => String.fromCharCode(byte)).join(''));
}

function isUuidColumn(column) {
    return /uuid|guid/i.test(column.name) || /uuid|binary\s*\(\s*16\s*\)/i.test(column.type);
}

// 16 byte blobs are UUIDs when the column says so, or when they carry RFC 9562 version and variant
// bits, so other 16 byte values such as MD5 hashes stay hex
function isUuidBytes(bytes, column) {
    if (bytes.length !== 16) return false;
    if (column && isUuidColumn(column)) return true;
    const version = bytes[6] >> 4;
    return version >= 1 && version <= 8 && (bytes[8] & 0xc0) === 0x80;
}

function formatBlob(base64, column) {
    const bytes = Uint8Array.fromBase64(base64);
    const hex = hexFromBytes(bytes);
    if (!isUuidBytes(bytes, column)) return hex;
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

// Accepts a UUID with or without dashes, or hex bytes with an optional 0x prefix and spaces
function parseBlob(text) {
    const value = text.trim();
    const hex = UUID_PATTERN.test(value) ? value.replace(/-/g, '') : value.replace(/^0x/i, '').replace(/\s+/g, '');
    if (hex.length % 2 !== 0 || !/^[0-9a-f]*$/i.test(hex)) return null;
    return Uint8Array.from(hex.match(/../g) || [], (byte) => parseInt(byte, 16));
}

// A time ordered UUIDv7, which keeps new primary keys clustered in index order
function newUuid() {
    const bytes = crypto.getRandomValues(new Uint8Array(16));
    const time = Date.now();
    for (let index = 0; index < 6; index++) bytes[index] = Math.floor(time / 2 ** (8 * (5 - index))) & 0xff;
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    const hex = hexFromBytes(bytes);
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function formatSqlValue(cell, isMysql) {
    if (cell.kind === 'null') return 'NULL';
    if (cell.kind === 'integer' || cell.kind === 'float') return String(cell.value);
    if (cell.kind === 'blob') return `X'${hexFromBytes(Uint8Array.fromBase64(cell.value)).toUpperCase()}'`;
    // MySQL treats backslashes in string literals as escapes by default
    const value = isMysql ? String(cell.value).replace(/\\/g, '\\\\') : String(cell.value);
    return `'${value.replace(/'/g, "''")}'`;
}

function formatCellValue(cell, column) {
    if (cell.kind === 'null') return 'NULL';
    return cell.kind === 'blob' ? formatBlob(cell.value, column) : cell.value;
}

PetiteVue.createApp({
    dbPath: '',
    dbFileName: '',
    dbOpened: false,
    connectionKind: '',
    connectionLabel: '',
    connectionTab: 'sqlite',
    connectionError: '',
    activeModal: '',
    browserError: '',
    isConnecting: false,
    isSelectingDatabase: false,
    databases: [],
    currentDatabase: '',
    activeDatabase: '',
    mysqlUsers: [],
    assignableDatabases: [],
    isLoadingUsers: false,
    isSavingUser: false,
    usersError: '',
    editingUser: false,
    isCreatingUser: false,
    selectedUserKey: '',
    userForm: {
        user: '',
        host: '%',
        password: '',
        databases: [],
        oldUser: null,
        oldHost: null,
    },
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
    schemaSql: '',
    schemaTableName: '',
    schemaColumns: [],
    schemaIndexes: [],
    schemaError: '',
    schemaLoading: false,
    schemaSaving: false,
    newSchemaColumn: emptySchemaColumn(),
    newSchemaIndex: emptySchemaIndex(),
    queryText: '',
    isCustomQuery: false,
    isQueryRunning: false,
    currentOffset: 0,
    currentTotal: 0,
    currentCursor: null,
    isLoading: false,
    viewGeneration: 0,
    rowActionError: '',
    rawQueryText: '',
    rawQueryColumns: [],
    rawQueryRows: [],
    rawQueryStatus: 'Ready',
    rawQueryError: '',
    rawQueryRunning: false,
    rawQueryHasRun: false,
    rowDialogMode: 'add',
    rowDialogKeys: null,
    rowFields: [],
    rowDialogError: '',
    isSavingRow: false,
    rowMenu: null,
    tableMenu: null,
    connectionMenu: null,
    isRowActionRunning: false,
    selectedRows: [],
    selectionAnchor: null,
    sortColumn: null,
    sortDesc: false,
    filterDraft: emptyFilter(),
    filter: null,
    filterOperators: FILTER_OPERATORS,
    foreignKeyActions: FOREIGN_KEY_ACTIONS,
    schemaForeignKeys: [],
    newForeignKey: emptyForeignKey(),
    referencedColumns: [],
    form: { kind: '', title: '', submitLabel: '', name: '', withData: true, characterSet: '', collation: '' },
    formError: '',
    isSubmittingForm: false,
    sqlTransferRunning: false,

    async init() {
        // macOS has no native titlebar, so empty parts of the top bars drag the window
        if (isMacosBwebview) {
            document.addEventListener('mousedown', (event) => {
                if (
                    event.button !== 0 ||
                    !event.target.closest('.is-titlebar') ||
                    event.target.closest('button, input, select, textarea, a')
                )
                    return;
                ipcSend(event.detail === 2 ? 'windowDoubleClick' : 'windowDrag');
            });
        }
        document.addEventListener('focusin', (event) => {
            const dialog = this.modalElement();
            if (dialog && !dialog.contains(event.target)) dialog.focus();
        });
        for (const type of ['click', 'blur', 'resize']) {
            window.addEventListener(type, () => this.closeMenus());
        }
        window.addEventListener('keydown', (event) => {
            if (event.key === 'Escape') this.closeMenus();
        });
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
        if (typeof IntersectionObserver === 'function') {
            const observer = new IntersectionObserver(
                (entries) => {
                    if (entries[0].isIntersecting && this.currentTable && !this.isCustomQuery) {
                        this.loadMoreRows(this.currentTable);
                    }
                },
                { rootMargin: '200px' },
            );
            observer.observe(this.$refs.loadSentinel);
        } else {
            const load = () => this.loadVisibleRows();
            this.$refs.loadSentinel.parentElement.addEventListener('scroll', load, { passive: true });
            window.addEventListener('resize', load);
        }

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
        this.activeTab = 'data';
        this.$nextTick(() => this.loadVisibleRows());
    },

    showSchema() {
        if (this.currentTable !== null) this.activeTab = 'schema';
    },

    schemaColumnPayload(column) {
        return {
            ...column,
            name: column.name.trim(),
            type: column.type.trim(),
            defaultSql: (column.defaultSql || '').trim() || null,
        };
    },

    schemaColumnChanged(column) {
        return (
            column.name.trim() !== column.originalName ||
            column.type.trim() !== column.originalType ||
            column.nullable !== column.originalNullable ||
            (column.defaultSql || '').trim() !== column.originalDefaultSql ||
            column.primaryKey !== column.originalPrimaryKey ||
            column.autoIncrement !== column.originalAutoIncrement
        );
    },

    schemaIndexChanged(index) {
        return (
            index.name.trim() !== index.originalName ||
            index.unique !== index.originalUnique ||
            index.columns.join('\0') !== index.originalColumns.join('\0')
        );
    },

    // The optional confirmation is shown as a native dialog: { title, description, confirm }
    async applySchemaChange(change, confirmation = null) {
        if (!this.currentTable || this.schemaSaving) return false;
        if (
            confirmation &&
            !(await confirmAction(confirmation.title, confirmation.description, confirmation.confirm))
        ) {
            return false;
        }
        this.schemaSaving = true;
        this.schemaError = '';
        const table = this.currentTable;
        try {
            const data = await api(tableUrl(table, 'schema'), 'PUT', change);
            if (data.error) {
                this.schemaError = data.error;
                return false;
            }
            this.currentTable = data.table;
            preferences.set('tableName', data.table);
            await this.loadTables();
            await this.openTableView(data.table);
            this.activeTab = 'schema';
            return true;
        } catch (error) {
            this.schemaError = `Schema change failed: ${error.message}`;
            return false;
        } finally {
            this.schemaSaving = false;
        }
    },

    async renameSchemaTable() {
        const newName = this.schemaTableName.trim();
        if (!newName || newName === this.currentTable) return;
        await this.applySchemaChange(
            { action: 'renameTable', newName },
            {
                title: `Rename table ${this.currentTable}?`,
                description: `Queries and views that use the old name ${this.currentTable} will stop working.`,
                confirm: 'Rename',
            },
        );
    },

    async saveSchemaColumn(column) {
        if (!this.schemaColumnChanged(column)) return;
        const definitionChanged =
            column.type.trim() !== column.originalType ||
            column.nullable !== column.originalNullable ||
            (column.defaultSql || '').trim() !== column.originalDefaultSql ||
            column.primaryKey !== column.originalPrimaryKey ||
            column.autoIncrement !== column.originalAutoIncrement;
        await this.applySchemaChange(
            {
                action: 'updateColumn',
                oldName: column.originalName,
                column: this.schemaColumnPayload(column),
            },
            definitionChanged
                ? {
                      title: `Change the definition of ${column.originalName}?`,
                      description: 'The database may rewrite or validate every existing row.',
                      confirm: 'Change column',
                  }
                : null,
        );
    },

    async addSchemaColumn() {
        if (
            await this.applySchemaChange({
                action: 'addColumn',
                column: this.schemaColumnPayload(this.newSchemaColumn),
            })
        ) {
            this.newSchemaColumn = emptySchemaColumn();
        }
    },

    async dropSchemaColumn(column) {
        await this.applySchemaChange(
            { action: 'dropColumn', name: column.originalName },
            {
                title: `Delete column ${column.originalName}?`,
                description: 'All data in this column is removed. This cannot be undone.',
                confirm: 'Delete column',
            },
        );
    },

    async addSchemaIndex() {
        if (
            await this.applySchemaChange({
                action: 'addIndex',
                index: { ...this.newSchemaIndex, name: this.newSchemaIndex.name.trim() },
            })
        ) {
            this.newSchemaIndex = emptySchemaIndex();
        }
    },

    async saveSchemaIndex(index) {
        if (!this.schemaIndexChanged(index)) return;
        await this.applySchemaChange({
            action: 'updateIndex',
            oldName: index.originalName,
            index: { ...index, name: index.name.trim() },
        });
    },

    async dropSchemaIndex(index) {
        await this.applySchemaChange(
            { action: 'dropIndex', name: index.originalName },
            {
                title: `Delete index ${index.originalName}?`,
                description: 'Queries that use this index may become slower.',
                confirm: 'Delete index',
            },
        );
    },

    showQuery() {
        this.activeTab = 'query';
    },

    async importSql() {
        if (!this.dbOpened || this.sqlTransferRunning) return;
        if (this.connectionKind === 'mysql' && !this.currentDatabase) {
            await showMessage('No database selected', 'Select a MySQL database before importing SQL.', 'info');
            return;
        }
        const confirmed = await confirmAction(
            'Import a SQL file?',
            'The script runs against the current database and may modify or delete data.',
            'Choose file',
        );
        if (!confirmed) return;
        this.sqlTransferRunning = true;
        try {
            const { cancelled, error } = await ipcRequest('importSql');
            if (cancelled) return;
            if (error) {
                this.resetBrowser();
                await this.loadTables();
                await showMessage('SQL import failed', error);
                return;
            }
            this.resetBrowser();
            await this.loadTables();
            await showMessage('SQL import completed', 'The SQL file was imported successfully.', 'info');
        } finally {
            this.sqlTransferRunning = false;
        }
    },

    async exportSql() {
        if (!this.dbOpened || this.sqlTransferRunning) return;
        if (this.connectionKind === 'mysql' && !this.currentDatabase) {
            await showMessage('No database selected', 'Select a MySQL database before exporting SQL.', 'info');
            return;
        }
        this.sqlTransferRunning = true;
        try {
            const baseName = (this.currentDatabase || this.dbFileName || 'database')
                .replace(/\.(db|sqlite|sqlite3)$/i, '')
                .replace(/[^a-z0-9._-]+/gi, '_');
            const { cancelled, error } = await ipcRequest('exportSql', { fileName: `${baseName || 'database'}.sql` });
            if (cancelled) return;
            if (error) {
                await showMessage('SQL export failed', error);
                return;
            }
            await showMessage('SQL export completed', 'The SQL file was saved successfully.', 'info');
        } finally {
            this.sqlTransferRunning = false;
        }
    },

    // A row dialog field, new rows default primary keys to the database default
    rowField(column, cell) {
        const type = column.type.toUpperCase();
        // SQLite can store text in a blob column, which is then edited as text
        const isBlob = cell ? cell.kind === 'blob' || (cell.kind === 'null' && column.is_blob) : column.is_blob;
        const isUuid =
            isBlob &&
            (isUuidColumn(column) ||
                (cell && cell.kind === 'blob' && isUuidBytes(Uint8Array.fromBase64(cell.value), column)));
        let mode = column.is_primary_key ? 'default' : 'value';
        let value = '';
        if (cell) {
            mode = cell.kind === 'null' ? 'null' : 'value';
            value = cell.kind === 'null' ? '' : formatCellValue(cell, column);
        } else if (isUuid && column.is_primary_key) {
            // UUID keys have no database default, so a new row gets a fresh one
            mode = 'value';
            value = newUuid();
        }
        return {
            name: column.name,
            type: column.type,
            isBlob,
            isUuid,
            isPrimaryKey: column.is_primary_key,
            inputMode: /INT/.test(type) ? 'numeric' : /REAL|FLOAT|DOUBLE/.test(type) ? 'decimal' : 'text',
            mode,
            value,
            initialMode: mode,
            initialValue: value,
        };
    },

    openRowDialog(mode, row = null) {
        this.rowDialogMode = mode;
        this.rowDialogKeys = row ? this.rowKeys(row) : null;
        this.rowDialogError = '';
        this.rowFields = this.columns
            .map((column, colIdx) => [column, row ? row[colIdx] : null])
            .filter(([column]) => !this.isGeneratedColumn(column.name))
            .map(([column, cell]) => this.rowField(column, cell));
        this.openModal('row');
    },

    openAddRowDialog() {
        if (this.canAddRow()) this.openRowDialog('add');
    },

    openEditRowDialog() {
        if (!this.canEditSelectedRow()) return;
        this.rowMenu = null;
        this.openRowDialog('edit', this.rows[this.selectedRows[0]]);
    },

    closeRowDialog() {
        if (this.isSavingRow) return;
        if (this.activeModal === 'row') this.closeModal();
        this.rowFields = [];
        this.rowDialogError = '';
    },

    rowValueModes() {
        const modes = [
            { value: 'value', label: 'Value' },
            { value: 'null', label: 'NULL' },
        ];
        if (this.rowDialogMode === 'add') modes.push({ value: 'default', label: 'Default' });
        return modes;
    },

    canAddRow() {
        return this.isTableView() && this.columns.length > 0;
    },

    canEditSelectedRow() {
        return this.isTableView() && this.hasPrimaryKey() && this.selectedRows.length === 1 && !this.isRowActionRunning;
    },

    canDuplicateSelectedRows() {
        return this.isTableView() && this.selectedRows.length > 0 && !this.isRowActionRunning && !this.schemaLoading;
    },

    canDeleteSelectedRows() {
        return this.isTableView() && this.hasPrimaryKey() && this.selectedRows.length > 0 && !this.isRowActionRunning;
    },

    rowActionTitle(action) {
        if (action !== 'duplicate' && this.isTableView() && !this.hasPrimaryKey()) {
            return `Rows can only be ${action === 'edit' ? 'edited' : 'deleted'} in tables with a primary key`;
        }
        if (this.selectedRows.length === 0) return 'Select a row first';
        if (action === 'edit') return this.selectedRows.length === 1 ? 'Edit the selected row' : 'Select a single row';
        return this.rowMenuLabel(
            `${action === 'delete' ? 'Delete' : 'Duplicate'} the selected row`,
            `${action === 'delete' ? 'Delete' : 'Duplicate'} the selected rows`,
        );
    },

    menuStyle(menu) {
        return menu ? { left: `${menu.x}px`, top: `${menu.y}px` } : {};
    },

    menuColumnName() {
        const column = this.rowMenu && this.columns[this.rowMenu.colIdx];
        return column ? column.name : '';
    },

    foreignKeyOf(colIdx) {
        const column = this.columns[colIdx];
        return column && column.foreign_key ? column.foreign_key : null;
    },

    closeMenus() {
        // Hidden menus keep their elements, so drop the focus of a clicked item
        if (document.activeElement && document.activeElement.closest('.menu')) document.activeElement.blur();
        if (!this.isRowActionRunning) this.rowMenu = null;
        this.tableMenu = null;
        this.connectionMenu = null;
    },

    connectionTitle() {
        if (!this.dbOpened) return 'Connect…';
        if (this.connectionKind === 'sqlite') return this.dbFileName;
        return this.currentDatabase || 'Select a database';
    },

    // Opens the connection dialog when nothing is open, otherwise the connection menu below the button
    toggleConnectionMenu(event) {
        if (!this.dbOpened) {
            this.openDatabase(event);
            return;
        }
        if (this.connectionMenu) {
            this.closeMenus();
            return;
        }
        const rect = event.currentTarget.getBoundingClientRect();
        this.closeMenus();
        this.connectionMenu = { x: rect.left, y: rect.bottom + 4 };
    },

    async chooseDatabase(database) {
        this.closeMenus();
        if (database !== this.currentDatabase) await this.selectDatabase(database);
    },

    hasDatabaseView() {
        return this.dbOpened && (this.connectionKind === 'sqlite' || this.currentDatabase !== '');
    },

    isTableView() {
        return this.currentTable !== null && !this.isCustomQuery;
    },

    hasPrimaryKey() {
        return this.columns.some((column) => column.is_primary_key);
    },

    isRowSelected(rowIdx) {
        return this.selectedRows.includes(rowIdx);
    },

    // Also closes the row menu, which points at rows that are being cleared or replaced
    clearSelection() {
        this.selectedRows = [];
        this.selectionAnchor = null;
        this.rowMenu = null;
    },

    selectRow(rowIdx, event) {
        if (event.target.closest('button, input')) return;
        if (event.shiftKey && this.selectionAnchor !== null) {
            const start = Math.min(this.selectionAnchor, rowIdx);
            const end = Math.max(this.selectionAnchor, rowIdx);
            this.selectedRows = Array.from({ length: end - start + 1 }, (_, index) => start + index);
            return;
        }
        if (event.metaKey || event.ctrlKey) {
            this.selectedRows = this.isRowSelected(rowIdx)
                ? this.selectedRows.filter((index) => index !== rowIdx)
                : [...this.selectedRows, rowIdx];
        } else {
            this.selectedRows = [rowIdx];
        }
        this.selectionAnchor = rowIdx;
    },

    onDataKeydown(event) {
        if (this.activeModal || event.target.closest('input, select, textarea')) return;
        const command = event.metaKey || event.ctrlKey;
        if (command && event.key.toLowerCase() === 'a') {
            event.preventDefault();
            this.selectedRows = this.rows.map((_, index) => index);
        } else if (command && event.key.toLowerCase() === 'c' && this.selectedRows.length > 0) {
            event.preventDefault();
            this.copySelectedRows();
        } else if (event.key === 'Enter' && this.canEditSelectedRow()) {
            event.preventDefault();
            this.openEditRowDialog();
        } else if ((event.key === 'Delete' || event.key === 'Backspace') && this.selectedRows.length > 0) {
            event.preventDefault();
            this.deleteSelectedRows();
        } else if (event.key === 'Escape') {
            this.clearSelection();
        }
    },

    sortedSelection() {
        return [...this.selectedRows].sort((a, b) => a - b).filter((index) => index < this.rows.length);
    },

    rowKeys(row) {
        return Object.fromEntries(
            this.columns
                .map((column, index) => [column, row[index]])
                .filter(([column]) => column.is_primary_key)
                .map(([column, value]) => [column.name, value]),
        );
    },

    openRowMenu(rowIdx, colIdx, event) {
        if (this.columns.length === 0) return;
        event.preventDefault();
        if (!this.isRowSelected(rowIdx)) {
            this.selectedRows = [rowIdx];
            this.selectionAnchor = rowIdx;
        }
        this.tableMenu = null;
        this.rowMenu = {
            colIdx,
            rowIdx,
            x: Math.max(8, Math.min(event.clientX, window.innerWidth - 248)),
            y: Math.max(8, Math.min(event.clientY, window.innerHeight - 300)),
        };
    },

    rowMenuLabel(singular, plural) {
        return this.selectedRows.length > 1 ? `${plural} (${this.selectedRows.length})` : singular;
    },

    canSetMenuCellNull() {
        const menu = this.rowMenu;
        if (!menu || !this.canWriteCell(menu.colIdx) || !this.rows[menu.rowIdx]) return false;
        const cell = this.rows[menu.rowIdx][menu.colIdx];
        const schemaColumn = this.schemaColumns.find((column) => column.name === this.columns[menu.colIdx].name);
        return cell.kind !== 'null' && (!schemaColumn || schemaColumn.nullable);
    },

    async copySelectedRows() {
        if (this.selectedRows.length === 0) return;
        const text = this.sortedSelection()
            .map((rowIdx) => this.rows[rowIdx].map((cell, colIdx) => this.formatCellValue(cell, colIdx)).join('\t'))
            .join('\n');
        this.rowMenu = null;
        await navigator.clipboard.writeText(text);
    },

    async copySelectedRowsAsInsert() {
        if (!this.isTableView() || this.selectedRows.length === 0) return;
        const columns = this.columns.map((column) => this.quoteIdentifier(column.name)).join(', ');
        const values = this.sortedSelection()
            .map((rowIdx) => `(${this.rows[rowIdx].map((cell) => this.formatSqlValue(cell)).join(', ')})`)
            .join(',\n    ');
        this.rowMenu = null;
        await navigator.clipboard.writeText(
            `INSERT INTO ${this.quoteIdentifier(this.currentTable)} (${columns}) VALUES\n    ${values};`,
        );
    },

    async setMenuCellNull() {
        const menu = this.rowMenu;
        if (!menu) return;
        this.rowMenu = null;
        await this.saveCell(menu.rowIdx, menu.colIdx, { kind: 'null', value: null });
    },

    // Runs a write against the current table and reloads it, keeping the sort and filter
    async runRowAction(label, request) {
        if (this.isRowActionRunning || !this.isTableView()) return;
        const table = this.currentTable;
        const generation = this.viewGeneration;
        this.isRowActionRunning = true;
        this.rowActionError = '';
        try {
            const data = await request(table);
            if (generation !== this.viewGeneration || table !== this.currentTable) return;
            if (data.error) throw new Error(data.error);
            this.rowMenu = null;
            await this.reloadTableData();
        } catch (error) {
            if (generation !== this.viewGeneration || table !== this.currentTable) return;
            this.rowActionError = `Could not ${label}: ${error.message}`;
        } finally {
            this.isRowActionRunning = false;
            this.rowMenu = null;
        }
    },

    async deleteSelectedRows() {
        if (!this.canDeleteSelectedRows()) return;
        const rows = this.sortedSelection().map((rowIdx) => this.rowKeys(this.rows[rowIdx]));
        this.rowMenu = null;
        const confirmed = await confirmAction(
            rows.length === 1
                ? `Delete this row from ${this.currentTable}?`
                : `Delete ${rows.length} rows from ${this.currentTable}?`,
            'This cannot be undone.',
            rows.length === 1 ? 'Delete row' : 'Delete rows',
        );
        if (!confirmed) return;
        await this.runRowAction('delete rows', (table) => api(tableUrl(table, 'data'), 'DELETE', { rows }));
    },

    // Copies the selected rows, letting the database generate auto increment and rowid keys
    async duplicateSelectedRows() {
        if (!this.canDuplicateSelectedRows()) return;
        const primaryKeys = this.schemaColumns.filter((column) => column.primaryKey);
        const generatedKey = (column) =>
            column.autoIncrement ||
            (this.connectionKind === 'sqlite' && primaryKeys.length === 1 && column.type.toUpperCase() === 'INTEGER');
        const generated = new Set(primaryKeys.filter(generatedKey).map((column) => column.name));
        for (const column of this.schemaColumns) {
            if (column.generated) generated.add(column.name);
        }
        const rows = this.sortedSelection().map((rowIdx) =>
            Object.fromEntries(
                this.columns.map((column, colIdx) => [
                    column.name,
                    generated.has(column.name) ? null : this.rows[rowIdx][colIdx],
                ]),
            ),
        );
        if (rows.length === 0) return;
        await this.runRowAction('duplicate rows', (table) => api(tableUrl(table, 'data'), 'POST', { rows }));
    },

    newRowValue(field) {
        if (field.mode === 'default') return null;
        if (field.mode === 'null') return { kind: 'null', value: null };
        const type = field.type.toUpperCase();
        if (field.isBlob) {
            const bytes = parseBlob(field.value);
            if (!bytes) throw new Error(`${field.name} must be a UUID or hex bytes`);
            if (field.isUuid && bytes.length !== 16) throw new Error(`${field.name} must be a 16 byte UUID`);
            return { kind: 'blob', value: base64FromBytes(bytes) };
        }
        if (/INT/.test(type)) {
            if (!/^[-+]?\d+$/.test(field.value.trim())) throw new Error(`${field.name} must be an integer`);
            return { kind: 'integer', value: field.value.trim() };
        }
        if (/REAL|FLOAT|DOUBLE/.test(type)) {
            const value = Number(field.value);
            if (field.value.trim() === '' || !Number.isFinite(value)) {
                throw new Error(`${field.name} must be a number`);
            }
            return { kind: 'float', value };
        }
        return { kind: 'text', value: field.value };
    },

    // Inserts a new row, or writes only the changed columns of the edited row
    async saveRowDialog() {
        if (this.isSavingRow || !this.currentTable) return;
        const isAdd = this.rowDialogMode === 'add';
        this.rowDialogError = '';
        let values;
        try {
            const fields = isAdd
                ? this.rowFields
                : this.rowFields.filter(
                      (field) => field.mode !== field.initialMode || field.value !== field.initialValue,
                  );
            values = Object.fromEntries(fields.map((field) => [field.name, this.newRowValue(field)]));
        } catch (error) {
            this.rowDialogError = error.message;
            return;
        }
        if (!isAdd && Object.keys(values).length === 0) {
            this.closeRowDialog();
            return;
        }

        this.isSavingRow = true;
        const table = this.currentTable;
        const generation = this.viewGeneration;
        try {
            const data = isAdd
                ? await api(tableUrl(table, 'data'), 'POST', { rows: [values] })
                : await api(tableUrl(table, 'data'), 'PUT', { values, keys: this.rowDialogKeys });
            if (generation !== this.viewGeneration || table !== this.currentTable) return;
            if (data.error) {
                this.rowDialogError = data.error;
                return;
            }
            this.isSavingRow = false;
            this.closeRowDialog();
            await this.reloadTableData();
        } catch (error) {
            if (generation !== this.viewGeneration || table !== this.currentTable) return;
            this.rowDialogError = `Could not save row: ${error.message}`;
        } finally {
            if (generation === this.viewGeneration && table === this.currentTable) {
                this.isSavingRow = false;
            }
        }
    },

    async openDatabase(event) {
        if (this.sqlTransferRunning || this.schemaSaving) return;
        this.connectionError = '';
        this.openConnectionDialog(event && event.currentTarget);
    },

    modalElement() {
        return this.activeModal ? this.$refs[`${this.activeModal}Dialog`] : null;
    },

    openModal(name, opener = document.activeElement) {
        if (this.activeModal) return false;
        modalFocus = opener;
        this.activeModal = name;
        this.closeMenus();
        this.$nextTick(() => {
            const dialog = this.modalElement();
            const input = dialog.querySelector('input:not([type="radio"]):not([type="checkbox"]), select');
            (input && input.getClientRects().length > 0 && name !== 'connection' ? input : dialog).focus();
        });
        return true;
    },

    closeModal() {
        if (!this.activeModal) return;
        this.activeModal = '';
        const previousFocus = modalFocus;
        modalFocus = null;
        this.$nextTick(() => {
            if (previousFocus && document.contains(previousFocus)) previousFocus.focus();
        });
    },

    // Closes the active dialog from its close button, the backdrop, or the Escape key
    // A click on the backdrop only closes the dialog when the press also started there, so drag
    // selecting text in a dialog input and releasing outside of it keeps the dialog open
    onBackdropMouseDown(event) {
        backdropPressed = event.target === event.currentTarget;
    },

    onBackdropClick() {
        if (backdropPressed) this.dismissModal();
        backdropPressed = false;
    },

    dismissModal() {
        if (this.activeModal === 'connection') this.closeConnectionDialog();
        if (this.activeModal === 'row') this.closeRowDialog();
        if (this.activeModal === 'users') this.closeUsersDialog();
        if (this.activeModal === 'form') this.closeFormDialog();
    },

    handleModalKeydown(event) {
        if (event.key === 'Escape') {
            event.preventDefault();
            event.stopPropagation();
            this.dismissModal();
        } else if (event.key === 'Tab') {
            const dialog = this.modalElement();
            const controls = Array.from(
                dialog.querySelectorAll('button, input, select, textarea, a[href], [tabindex]'),
            ).filter((element) => !element.disabled && element.tabIndex >= 0 && element.getClientRects().length > 0);
            const first = controls[0] || dialog;
            const last = controls[controls.length - 1] || dialog;
            if (document.activeElement === dialog || document.activeElement === (event.shiftKey ? first : last)) {
                event.preventDefault();
                (event.shiftKey ? last : first).focus();
            }
        }
    },

    openConnectionDialog(opener = document.activeElement) {
        this.openModal('connection', opener);
    },

    closeConnectionDialog() {
        if (this.activeModal === 'connection') this.closeModal();
    },

    async openUsersDialog() {
        if (this.connectionKind !== 'mysql') return;
        this.usersError = '';
        this.cancelEditingUser();
        if (this.openModal('users')) await this.loadUsers();
    },

    closeUsersDialog() {
        if (this.isSavingUser) return;
        if (this.activeModal === 'users') this.closeModal();
        this.cancelEditingUser();
    },

    async loadUsers() {
        this.isLoadingUsers = true;
        this.usersError = '';
        try {
            const data = await api('/api/users');
            if (data.error) {
                this.mysqlUsers = [];
                this.usersError = data.error;
                return;
            }
            this.mysqlUsers = data;
        } catch (error) {
            this.mysqlUsers = [];
            this.usersError = `Failed to load users: ${error.message}`;
        } finally {
            this.isLoadingUsers = false;
        }
    },

    availableUserDatabases(extra = []) {
        const systemDatabases = new Set(['information_schema', 'mysql', 'performance_schema', 'sys']);
        return [...new Set([...this.databases.filter((database) => !systemDatabases.has(database)), ...extra])].sort();
    },

    startCreatingUser() {
        this.usersError = '';
        this.editingUser = true;
        this.isCreatingUser = true;
        this.selectedUserKey = '';
        this.assignableDatabases = this.availableUserDatabases();
        this.userForm = {
            user: '',
            host: '%',
            password: '',
            databases:
                this.currentDatabase && this.assignableDatabases.includes(this.currentDatabase)
                    ? [this.currentDatabase]
                    : [],
            oldUser: null,
            oldHost: null,
        };
    },

    startEditingUser(account) {
        this.usersError = '';
        this.editingUser = true;
        this.isCreatingUser = false;
        this.selectedUserKey = `${account.user}@${account.host}`;
        this.assignableDatabases = this.availableUserDatabases(account.databases);
        this.userForm = {
            user: account.user,
            host: account.host,
            password: '',
            databases: [...account.databases],
            oldUser: account.user,
            oldHost: account.host,
        };
    },

    cancelEditingUser() {
        this.editingUser = false;
        this.isCreatingUser = false;
        this.selectedUserKey = '';
        this.usersError = '';
    },

    async saveUser() {
        if (this.isSavingUser) return;
        this.isSavingUser = true;
        this.usersError = '';
        try {
            const data = await api('/api/users', this.isCreatingUser ? 'POST' : 'PUT', this.userForm);
            if (data.error) {
                const saveError = data.error;
                if (data.reload) {
                    const oldKey = `${this.userForm.oldUser}@${this.userForm.oldHost}`;
                    const newKey = `${this.userForm.user}@${this.userForm.host}`;
                    await this.loadUsers();
                    const currentUser = this.mysqlUsers.find((account) => {
                        const key = `${account.user}@${account.host}`;
                        return key === oldKey || key === newKey;
                    });
                    if (currentUser) this.startEditingUser(currentUser);
                }
                this.usersError = saveError;
                return;
            }
            const savedKey = `${this.userForm.user}@${this.userForm.host}`;
            await this.loadUsers();
            const savedUser = this.mysqlUsers.find((account) => `${account.user}@${account.host}` === savedKey);
            if (savedUser) this.startEditingUser(savedUser);
        } catch (error) {
            this.usersError = `Failed to save user: ${error.message}`;
        } finally {
            this.isSavingUser = false;
        }
    },

    async deleteUser() {
        if (this.isCreatingUser || this.isSavingUser) return;
        const account = `${this.userForm.oldUser}@${this.userForm.oldHost}`;
        const confirmed = await confirmAction(
            `Delete ${account}?`,
            'This immediately removes the account and all of its grants.',
            'Delete user',
        );
        if (!confirmed) return;
        this.isSavingUser = true;
        this.usersError = '';
        try {
            const data = await api('/api/users', 'DELETE', {
                user: this.userForm.oldUser,
                host: this.userForm.oldHost,
            });
            if (data.error) {
                this.usersError = data.error;
                return;
            }
            this.cancelEditingUser();
            await this.loadUsers();
        } catch (error) {
            this.usersError = `Failed to delete user: ${error.message}`;
        } finally {
            this.isSavingUser = false;
        }
    },

    userInitial(user) {
        return (user || '?').slice(0, 1).toUpperCase();
    },

    databaseAccessLabel(account) {
        if (account.databases.length === 0) return 'No full direct grants';
        if (account.databases.length === 1) return `Full access to ${account.databases[0]}`;
        return `${account.databases.length} full direct grants`;
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
            await showMessage('Failed to open database', error);
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
            if (!(await this._openMysql(true))) this.openConnectionDialog();
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
                if (savedConnection && savedConnection.transport && savedConnection.user) {
                    previousConnection = savedConnection;
                }
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
            await showMessage(
                'Connected, but credentials could not be saved securely',
                response.credentialError,
                'warning',
            );
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
            const data = await api('/api/databases');
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
        if (!database || this.isSelectingDatabase || this.sqlTransferRunning || this.schemaSaving) return;
        this.isSelectingDatabase = true;
        let response;
        try {
            response = await ipcRequest('selectMysqlDatabase', { database });
        } finally {
            this.isSelectingDatabase = false;
        }
        if (!response.ok) {
            this.currentDatabase = this.activeDatabase;
            await showMessage('Failed to open database', response.error);
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
        if (this.activeModal !== 'connection') this.closeModal();
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
        this.schemaSql = '';
        this.schemaTableName = '';
        this.schemaColumns = [];
        this.schemaIndexes = [];
        this.schemaError = '';
        this.schemaLoading = false;
        this.schemaSaving = false;
        this.newSchemaColumn = emptySchemaColumn();
        this.newSchemaIndex = emptySchemaIndex();
        this.queryText = '';
        this.isCustomQuery = false;
        this.activeTab = 'data';
        this.showDataTable = false;
        this.showDataLoading = false;
        this.showDataEmpty = false;
        this.browserError = '';
        this.rowActionError = '';
        rawQueryGeneration += 1;
        this.rawQueryText = '';
        this.rawQueryColumns = [];
        this.rawQueryRows = [];
        this.rawQueryStatus = 'Ready';
        this.rawQueryError = '';
        this.rawQueryRunning = false;
        this.rawQueryHasRun = false;
        this.rowFields = [];
        this.rowDialogError = '';
        this.isSavingRow = false;
        this.rowMenu = null;
        this.tableMenu = null;
        this.isRowActionRunning = false;
        this.clearSelection();
        this.sortColumn = null;
        this.sortDesc = false;
        this.filter = null;
        this.filterDraft = emptyFilter();
        this.schemaForeignKeys = [];
        this.newForeignKey = emptyForeignKey();
        this.referencedColumns = [];
    },

    async loadTables() {
        const generation = this.viewGeneration;
        try {
            const data = await api('/api/tables');
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
        if (name === this.currentTable || this.schemaSaving) return;
        this.currentTable = name;
        this.isCustomQuery = false;
        this.queryText = '';
        preferences.set('tableName', name);
        await this.openTableView(name);
    },

    selectTableFromList(name) {
        this.$refs.tablesList.focus({ preventScroll: true });
        this.selectTable(name);
    },

    onTableListKeydown(event) {
        if (!['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key) || this.tables.length === 0) return;
        event.preventDefault();

        const currentIndex = this.tables.indexOf(this.currentTable);
        let targetIndex;
        if (event.key === 'Home') {
            targetIndex = 0;
        } else if (event.key === 'End') {
            targetIndex = this.tables.length - 1;
        } else if (event.key === 'ArrowUp') {
            targetIndex = currentIndex < 0 ? this.tables.length - 1 : Math.max(0, currentIndex - 1);
        } else {
            targetIndex = currentIndex < 0 ? 0 : Math.min(this.tables.length - 1, currentIndex + 1);
        }

        this.selectTable(this.tables[targetIndex]);
        const option = document.getElementById(`table-option-${targetIndex}`);
        if (option) option.scrollIntoView({ block: 'nearest' });
    },

    async openTableView(name) {
        this.sortColumn = null;
        this.sortDesc = false;
        this.filter = null;
        this.filterDraft = emptyFilter();
        this.activeTab = 'data';
        await this.reloadTable(name);
    },

    // Reloads the rows and schema of a table, keeping the active tab, sort, and filter
    async reloadTable(name = this.currentTable) {
        const generation = ++this.viewGeneration;
        document.title = `Sequel Explorer - ${this.dbFileName} - ${name}`;

        this.isLoading = false;
        this.isQueryRunning = false;
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
        this.rowActionError = '';
        this.clearSelection();
        this.schemaSql = '';
        this.schemaTableName = name;
        this.schemaColumns = [];
        this.schemaIndexes = [];
        this.schemaForeignKeys = [];
        this.schemaError = '';
        this.schemaLoading = true;

        await this.loadMoreRows(name, generation);
        if (generation !== this.viewGeneration || name !== this.currentTable) return;

        try {
            const data = await api(tableUrl(name, 'schema'));
            if (generation !== this.viewGeneration || name !== this.currentTable) return;
            if (data.error) {
                this.schemaError = data.error;
                return;
            }
            this.schemaSql = data.sql || '';
            this.schemaTableName = data.name;
            this.schemaColumns = data.columns.map((column) => ({
                ...column,
                originalName: column.name,
                defaultSql: column.defaultSql || '',
                originalType: column.type,
                originalNullable: column.nullable,
                originalDefaultSql: column.defaultSql || '',
                originalPrimaryKey: column.primaryKey,
                originalAutoIncrement: column.autoIncrement,
            }));
            this.schemaIndexes = data.indexes.map((index) => ({
                ...index,
                originalName: index.name,
                originalUnique: index.unique,
                originalColumns: [...index.columns],
            }));
            this.schemaForeignKeys = data.foreignKeys;
            this.schemaError = '';
        } catch (error) {
            if (generation !== this.viewGeneration || name !== this.currentTable) return;
            this.schemaError = 'Error loading schema: ' + error.message;
        } finally {
            if (generation === this.viewGeneration && name === this.currentTable) this.schemaLoading = false;
        }
    },

    // Reloads only the rows, used after row writes so the schema does not flicker
    async reloadTableData() {
        const name = this.currentTable;
        if (name === null) return;
        const generation = ++this.viewGeneration;
        this.isLoading = false;
        this.currentOffset = 0;
        this.currentTotal = 0;
        this.currentCursor = null;
        this.rows = [];
        this.showDataEmpty = false;
        this.clearSelection();
        await this.loadMoreRows(name, generation);
    },

    toggleSort(colIdx) {
        if (!this.isTableView()) return;
        const name = this.columns[colIdx].name;
        if (this.sortColumn !== name) {
            this.sortColumn = name;
            this.sortDesc = false;
        } else if (!this.sortDesc) {
            this.sortDesc = true;
        } else {
            this.sortColumn = null;
            this.sortDesc = false;
        }
        this.reloadTableData();
    },

    sortIndicator(colIdx) {
        const column = this.columns[colIdx];
        if (!this.isTableView() || !column || column.name !== this.sortColumn) return '';
        return this.sortDesc ? '▼' : '▲';
    },

    filterNeedsValue() {
        return this.filterDraft.operator !== 'isNull' && this.filterDraft.operator !== 'isNotNull';
    },

    applyFilter() {
        const { column, operator, value } = this.filterDraft;
        if (!column) return this.clearFilter();
        this.filter = { column, operator, value: this.filterNeedsValue() ? value : '' };
        this.reloadTableData();
    },

    clearFilter() {
        this.filterDraft = emptyFilter();
        if (this.filter === null) return;
        this.filter = null;
        this.reloadTableData();
    },

    async refresh() {
        if (!this.dbOpened || this.isRowActionRunning || this.schemaSaving) return;
        if (this.connectionKind === 'mysql') await this.loadDatabases();
        if (this.connectionKind === 'mysql' && !this.currentDatabase) return;
        await this.loadTables();
        if (this.currentTable !== null && !this.tables.includes(this.currentTable)) {
            this.clearTableView();
        } else if (this.isCustomQuery) {
            await this.runDataQuery();
        } else if (this.currentTable !== null) {
            await this.reloadTable();
        }
    },

    clearTableView() {
        this.viewGeneration += 1;
        this.currentTable = null;
        this.isCustomQuery = false;
        this.isLoading = false;
        this.isQueryRunning = false;
        this.showDataLoading = false;
        this.showDataTable = false;
        this.showDataEmpty = false;
        this.rows = [];
        this.columns = [];
        this.rowCount = '';
        this.clearSelection();
        if (this.activeTab === 'schema') this.activeTab = 'data';
        document.title = `Sequel Explorer - ${this.dbFileName}`;
    },

    loadVisibleRows() {
        if (
            typeof IntersectionObserver === 'function' ||
            !this.currentTable ||
            this.isCustomQuery ||
            this.showDataEmpty
        )
            return;
        const sentinel = this.$refs.loadSentinel;
        if (sentinel.getClientRects().length === 0) return;
        const bounds = sentinel.parentElement.getBoundingClientRect();
        const rect = sentinel.getBoundingClientRect();
        if (rect.top <= bounds.bottom + 200 && rect.bottom >= bounds.top - 200) {
            this.loadMoreRows(this.currentTable);
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
            if (this.sortColumn !== null) {
                params.set('sort', this.sortColumn);
                params.set('desc', this.sortDesc);
            }
            if (this.filter !== null) {
                params.set('filter_column', this.filter.column);
                params.set('filter_operator', this.filter.operator);
                params.set('filter_value', this.filter.value);
            }
            const data = await api(`${tableUrl(tableName, 'data')}?${params}`);
            if (generation !== this.viewGeneration || tableName !== this.currentTable) return;

            if (data.error) {
                this.dataEmptyText = 'Error: ' + data.error;
                this.showDataEmpty = true;
                return;
            }

            if (offset === 0) {
                this.currentTotal = data.total;
                this.rowCount = `${data.total.toLocaleString()} ${this.filter !== null ? 'matching ' : ''}rows`;
                this.columns = data.columns;
                this.showDataTable = true;
                if (data.rows.length === 0) {
                    this.dataEmptyText = this.filter !== null ? 'No rows match the filter' : 'No rows';
                    this.showDataEmpty = true;
                    return;
                }
            }

            this.appendRows(data.rows);
            this.currentOffset = offset + data.rows.length;
            this.currentCursor = data.next_cursor;
            if (data.rows.length > 0) this.$nextTick(() => this.loadVisibleRows());
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

    // Cells of key tables can be written, except generated columns
    canWriteCell(colIdx) {
        const column = this.columns[colIdx];
        return (
            this.isTableView() && column !== undefined && !this.isGeneratedColumn(column.name) && this.hasPrimaryKey()
        );
    },

    isGeneratedColumn(name) {
        return this.schemaColumns.some((column) => column.originalName === name && column.generated);
    },

    // Double clicking a row opens it in the edit dialog
    editRowAt(rowIdx, event) {
        if (event.target.closest('button')) return;
        this.selectedRows = [rowIdx];
        this.selectionAnchor = rowIdx;
        this.openEditRowDialog();
    },

    async saveCell(rowIdx, colIdx, value) {
        const generation = this.viewGeneration;
        const table = this.currentTable;
        const column = this.columns[colIdx];
        const row = this.rows[rowIdx];
        try {
            const data = await api(tableUrl(table, 'data'), 'PUT', {
                values: { [column.name]: value },
                keys: this.rowKeys(row),
            });
            if (generation !== this.viewGeneration || table !== this.currentTable) return;
            if (data.error) {
                this.rowActionError = `Could not save ${column.name}: ${data.error}`;
                return;
            }
            // Changing a key moves the row in the key ordered view, so reload instead of patching
            if (column.is_primary_key) {
                await this.reloadTableData();
                return;
            }
            row[colIdx] = value;
            this.rows = [...this.rows];
        } catch (error) {
            if (generation !== this.viewGeneration || table !== this.currentTable) return;
            this.rowActionError = `Could not save ${column.name}: ${error.message}`;
        }
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
        await this.runDataQuery();
    },

    runQuery() {
        return this.activeTab === 'query' ? this.runRawQuery() : this.runDataQuery();
    },

    async runDataQuery() {
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
        this.clearSelection();

        try {
            const data = await ipcRequest('runQuery', { sql });
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
        if (this.currentTable) {
            this.openTableView(this.currentTable);
        } else {
            this.clearTableView();
        }
    },

    onRawQueryKeydown(event) {
        if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
            event.preventDefault();
            this.runRawQuery();
        }
    },

    async runRawQuery() {
        const sql = this.rawQueryText.trim();
        if (!sql || this.rawQueryRunning) return;

        this.rawQueryRunning = true;
        this.rawQueryHasRun = true;
        this.rawQueryError = '';
        this.rawQueryColumns = [];
        this.rawQueryRows = [];
        this.rawQueryStatus = 'Running...';
        const generation = ++rawQueryGeneration;
        const startedAt = performance.now();
        let changed = false;
        try {
            const data = await ipcRequest('runRawQuery', { sql });
            if (generation !== rawQueryGeneration) return;
            const elapsed = Math.max(1, Math.round(performance.now() - startedAt));
            if (data.error) {
                this.rawQueryError = data.error;
                this.rawQueryStatus = `Failed in ${elapsed} ms`;
                return;
            }

            this.rawQueryColumns = data.columns;
            this.rawQueryRows = data.rows;
            const parts = [];
            if (data.statements > 1) parts.push(`${data.statements} statements`);
            if (data.columns.length > 0) {
                parts.push(`${data.rows.length.toLocaleString()}${data.truncated ? '+' : ''} rows`);
            }
            if (data.affectedRows > 0) parts.push(`${data.affectedRows.toLocaleString()} rows affected`);
            if (parts.length === 0) parts.push('Statement executed');
            this.rawQueryStatus = `${parts.join(', ')} in ${elapsed} ms`;
            changed = data.affectedRows > 0 || data.columns.length === 0;
        } catch (error) {
            if (generation !== rawQueryGeneration) return;
            const elapsed = Math.max(1, Math.round(performance.now() - startedAt));
            this.rawQueryError = error.message;
            this.rawQueryStatus = `Failed in ${elapsed} ms`;
        } finally {
            if (generation === rawQueryGeneration) this.rawQueryRunning = false;
        }
        // Writes and schema changes make the browser stale
        if (changed) await this.refreshBrowserAfterWrite();
    },

    async refreshBrowserAfterWrite() {
        await this.loadTables();
        if (this.currentTable !== null && !this.tables.includes(this.currentTable)) {
            this.clearTableView();
        } else if (this.currentTable !== null && !this.isCustomQuery) {
            await this.reloadTable();
        }
    },

    openTableMenu(name, event) {
        if (this.schemaSaving || this.sqlTransferRunning) return;
        event.preventDefault();
        this.rowMenu = null;
        this.tableMenu = {
            name,
            x: Math.max(8, Math.min(event.clientX, window.innerWidth - 248)),
            y: Math.max(8, Math.min(event.clientY, window.innerHeight - 160)),
        };
    },

    openFormDialog(form) {
        if (this.activeModal) return;
        this.formError = '';
        this.form = {
            name: '',
            withData: true,
            characterSet: '',
            collation: '',
            ...form,
        };
        this.openModal('form');
    },

    closeFormDialog() {
        if (this.isSubmittingForm) return;
        if (this.activeModal === 'form') this.closeModal();
    },

    openCreateTableDialog() {
        if (!this.dbOpened || (this.connectionKind === 'mysql' && !this.currentDatabase)) return;
        this.openFormDialog({ kind: 'createTable', title: 'New table', submitLabel: 'Create table' });
    },

    openCurrentTableMenu(event) {
        if (this.currentTable === null) return;
        const opened = this.tableMenu && this.tableMenu.name === this.currentTable;
        this.closeMenus();
        if (opened) return;
        const rect = event.currentTarget.getBoundingClientRect();
        this.tableMenu = {
            name: this.currentTable,
            x: Math.min(rect.left, window.innerWidth - 248),
            y: rect.bottom + 4,
        };
    },

    // Table actions apply to the right-clicked table, or to the selected table from other menus
    tableActionTarget() {
        const name = this.tableMenu ? this.tableMenu.name : this.currentTable;
        this.closeMenus();
        return this.schemaSaving || this.sqlTransferRunning ? null : name;
    },

    openDuplicateTableDialog() {
        const name = this.tableActionTarget();
        if (!name) return;
        this.openFormDialog({
            kind: 'duplicateTable',
            title: `Duplicate ${name}`,
            submitLabel: 'Duplicate',
            source: name,
            name: `${name}_copy`,
        });
    },

    truncateTable() {
        return this.runTableOperation('truncate');
    },

    dropTable() {
        return this.runTableOperation('drop');
    },

    openCreateDatabaseDialog() {
        if (this.connectionKind !== 'mysql') return;
        this.openFormDialog({
            kind: 'createDatabase',
            title: 'New database',
            submitLabel: 'Create database',
            characterSet: 'utf8mb4',
        });
    },

    async submitForm() {
        if (this.isSubmittingForm) return;
        const form = this.form;
        const name = form.name.trim();
        if (!name) {
            this.formError = 'A name is required';
            return;
        }
        this.isSubmittingForm = true;
        this.formError = '';
        try {
            let data;
            if (form.kind === 'createTable') {
                data = await api('/api/tables', 'POST', { name });
            } else if (form.kind === 'duplicateTable') {
                data = await api(tableUrl(form.source, 'operation'), 'POST', {
                    action: 'duplicate',
                    newName: name,
                    withData: form.withData,
                });
            } else {
                data = await api('/api/databases', 'POST', {
                    name,
                    characterSet: form.characterSet.trim(),
                    collation: form.collation.trim(),
                });
            }
            if (data.error) {
                this.formError = data.error;
                return;
            }
            this.isSubmittingForm = false;
            this.closeFormDialog();
            if (form.kind === 'createDatabase') {
                await this.loadDatabases();
                await this.selectDatabase(name);
            } else {
                await this.loadTables();
                await this.selectTable(data.table);
                if (form.kind === 'createTable') this.activeTab = 'schema';
            }
        } catch (error) {
            this.formError = error.message;
        } finally {
            this.isSubmittingForm = false;
        }
    },

    async runTableOperation(action) {
        const name = this.tableActionTarget();
        if (!name) return;
        const confirmations = {
            truncate: {
                title: `Truncate ${name}?`,
                description: 'All rows are removed and auto increment counters are reset. This cannot be undone.',
                confirm: 'Truncate table',
            },
            drop: {
                title: `Delete table ${name}?`,
                description: 'The table, its rows, indexes, and triggers are removed. This cannot be undone.',
                confirm: 'Delete table',
            },
        };
        const { title, description, confirm } = confirmations[action];
        if (!(await confirmAction(title, description, confirm))) return;
        const data = await api(tableUrl(name, 'operation'), 'POST', { action });
        if (data.error) {
            await showMessage(`Could not ${action} ${name}`, data.error);
            return;
        }
        await this.loadTables();
        if (name !== this.currentTable) return;
        if (action === 'drop') {
            this.clearTableView();
        } else {
            await this.reloadTableData();
        }
    },

    async dropCurrentDatabase() {
        this.closeMenus();
        const database = this.currentDatabase;
        if (this.connectionKind !== 'mysql' || !database) return;
        const confirmed = await confirmAction(
            `Delete database ${database}?`,
            'All tables and data in this database are removed. This cannot be undone.',
            'Delete database',
        );
        if (!confirmed) return;
        const data = await api('/api/databases', 'DELETE', { name: database });
        if (data.error) {
            await showMessage(`Could not delete ${database}`, data.error);
            return;
        }
        if (preferences.get('mysqlDatabase') === database) preferences.remove('mysqlDatabase');
        this.resetBrowser();
        this.currentDatabase = '';
        this.activeDatabase = '';
        await this.loadDatabases();
        if (this.databases.length > 0) await this.selectDatabase(this.databases[0]);
    },

    foreignKeyLabel(foreignKey) {
        const actions = [
            foreignKey.onDelete !== 'NO ACTION' ? `ON DELETE ${foreignKey.onDelete}` : '',
            foreignKey.onUpdate !== 'NO ACTION' ? `ON UPDATE ${foreignKey.onUpdate}` : '',
        ].filter(Boolean);
        const target = `${foreignKey.referencedTable}(${foreignKey.referencedColumns.join(', ') || 'primary key'})`;
        return `${foreignKey.columns.join(', ')} → ${target}${actions.length > 0 ? ` ${actions.join(' ')}` : ''}`;
    },

    async loadReferencedColumns() {
        const table = this.newForeignKey.referencedTable;
        this.referencedColumns = [];
        this.newForeignKey.referencedColumn = '';
        if (!table) return;
        const data = await api(tableUrl(table, 'schema'));
        if (data.error || table !== this.newForeignKey.referencedTable) return;
        this.referencedColumns = data.columns.map((column) => column.name);
        const primaryKey = data.columns.find((column) => column.primaryKey);
        this.newForeignKey.referencedColumn = primaryKey ? primaryKey.name : this.referencedColumns[0] || '';
    },

    async addSchemaForeignKey() {
        const foreignKey = this.newForeignKey;
        const added = await this.applySchemaChange({
            action: 'addForeignKey',
            foreignKey: {
                name: foreignKey.name.trim(),
                columns: [foreignKey.column],
                referencedTable: foreignKey.referencedTable,
                referencedColumns: [foreignKey.referencedColumn],
                onUpdate: foreignKey.onUpdate,
                onDelete: foreignKey.onDelete,
            },
        });
        if (added) {
            this.newForeignKey = { ...this.newForeignKey, name: '', column: '' };
        }
    },

    async dropSchemaForeignKey(foreignKey) {
        await this.applySchemaChange(
            { action: 'dropForeignKey', name: foreignKey.name },
            {
                title: 'Delete foreign key?',
                description: `${this.foreignKeyLabel(foreignKey)} will no longer be enforced.`,
                confirm: 'Delete foreign key',
            },
        );
    },

    // Render helpers can run for rows that are being replaced, so missing columns are tolerated
    formatRawQueryCell(cell, columnIdx) {
        return formatCellValue(cell, this.rawQueryColumns[columnIdx]);
    },

    formatCellValue(cell, colIdx) {
        return formatCellValue(cell, this.columns[colIdx]);
    },

    rowFieldPlaceholder(field) {
        if (field.isUuid) return '00000000-0000-0000-0000-000000000000';
        return field.isBlob ? 'Hex bytes, for example 0A1B2C' : `Enter ${field.name}`;
    },
}).mount('#app');
