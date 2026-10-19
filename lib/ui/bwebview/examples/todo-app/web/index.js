/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Polyfills
if (!crypto.randomUUID) {
    crypto.randomUUID = () =>
        'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
            const r = (Math.random() * 16) | 0;
            return (c === 'x' ? r : (r & 0x3) | 0x8).toString(16);
        });
}

window.addEventListener('contextmenu', (event) => event.preventDefault());

function ipcSend(type, data = {}) {
    window.ipc.postMessage(JSON.stringify({ type, ...data }));
}

// Requests of the same type are sent one at a time, so each response matches its request
const pendingRequests = {};
function ipcRequest(type, data = {}) {
    const response = (pendingRequests[type] || Promise.resolve()).then(
        () =>
            new Promise((resolve) => {
                const listener = (event) => {
                    const message = JSON.parse(event.data);
                    if (message.type === `${type}Response`) {
                        window.ipc.removeEventListener('message', listener);
                        resolve(message);
                    }
                };
                window.ipc.addEventListener('message', listener);
                ipcSend(type, data);
            }),
    );
    pendingRequests[type] = response.catch(() => {});
    return response;
}

PetiteVue.createApp({
    todos: [],
    input: '',

    addTodo(e) {
        e.preventDefault();
        if (this.input.trim() === '') return;

        this.todos.push({
            id: crypto.randomUUID(),
            text: this.input,
            completed: false,
        });
        this.input = '';
        this.saveTodos();
    },

    removeCompleted(e) {
        e.preventDefault();
        this.todos = this.todos.filter((todo) => !todo.completed);
        this.saveTodos();
    },

    completeTodo(index, completed) {
        this.todos[index].completed = completed;
        this.saveTodos();
    },

    async loadTodos() {
        const { todos } = await ipcRequest('getTodos');
        this.todos = todos;
    },

    saveTodos() {
        ipcSend('updateTodos', { todos: this.todos });
    },
}).mount('#app');
