import init, { WasmRegistry } from "@fury/wasm-core";
import { fetchLeaderboard } from "./wire.js";

const byId = (id) => document.getElementById(id);
const quiz = byId("quiz");
const dialog = byId("auth-dialog");
const auth = byId("auth-form");
const next = byId("next");
const note = byId("quiz-status");
const answers = [];
let handle = null;
let action = "login";
let questions = [];
let runtime;

async function request(path, data) {
    const response = await fetch(path, data === undefined ? {} : {
        method: "POST", headers: {"Content-Type": "application/json"}, body: JSON.stringify(data),
    });
    if (!response.ok) {
        const body = await response.json();
        throw new Error(typeof body.detail === "string" ? body.detail : "Check the submitted fields.");
    }
    return response;
}

async function bytes(path) {
    return new Uint8Array(await (await request(path)).arrayBuffer());
}

function setAccount(name) {
    handle = name;
    byId("account").textContent = name ?? "";
    byId("account").hidden = !name;
    byId("logout").hidden = !name;
    byId("login").hidden = !!name;
    byId("register").hidden = !!name;
}

function openAuth(mode) {
    action = mode;
    auth.reset();
    byId("auth-status").textContent = "";
    byId("auth-title").textContent = byId("auth-submit").textContent = mode === "login" ? "Log in" : "Create account";
    auth.elements.password.autocomplete = mode === "login" ? "current-password" : "new-password";
    dialog.showModal();
}

byId("login").onclick = () => openAuth("login");
byId("register").onclick = () => openAuth("register");
byId("close-auth").onclick = () => dialog.close();
byId("logout").onclick = async () => {
    try {
        await request("/api/logout", {});
        setAccount(null);
    } catch (error) {
        note.textContent = error.message;
    }
};

auth.onsubmit = async (event) => {
    event.preventDefault();
    byId("auth-submit").disabled = true;
    try {
        const data = Object.fromEntries(new FormData(auth));
        const profile = await (await request("/api/" + action, data)).json();
        setAccount(profile.handle);
        dialog.close();
        if (answers.length === questions.length) note.textContent = "You can save your score now.";
    } catch (error) {
        byId("auth-status").textContent = error.message;
    } finally {
        byId("auth-submit").disabled = false;
    }
};

function showQuestion() {
    const question = questions[answers.length];
    byId("meter").value = answers.length;
    byId("progress").textContent = "Question " + (answers.length + 1) + " of " + questions.length;
    byId("prompt").textContent = question.prompt;
    byId("choices").replaceChildren(...question.choices.map((text, index) => {
        const label = document.createElement("label");
        label.className = "choice";
        const input = document.createElement("input");
        input.type = "radio";
        input.name = "answer";
        input.value = index;
        input.required = true;
        label.append(input, text);
        return label;
    }));
    next.disabled = false;
    next.textContent = answers.length + 1 === questions.length ? "Submit answers" : "Next question";
}

async function showRanking() {
    try {
        runtime ??= (async () => {
            await init();
            return new WasmRegistry(await bytes("/registry.bin"));
        })().catch((error) => {
            runtime = undefined;
            throw error;
        });
        const rows = await fetchLeaderboard(await runtime);
        byId("rankings").replaceChildren(...rows.slice(0, 8).map(({handle, score}) => {
            const row = document.createElement("tr");
            row.insertCell().textContent = handle;
            row.insertCell().textContent = score + " / " + questions.length;
            return row;
        }));
        byId("ranking-status").textContent = rows.length ? "" : "No scores yet.";
    } catch (error) {
        byId("ranking-status").textContent = error.message;
    }
}

quiz.onsubmit = async (event) => {
    event.preventDefault();
    if (answers.length < questions.length) {
        const selected = new FormData(quiz).get("answer");
        if (selected === null) return;
        answers.push(Number(selected));
        if (answers.length < questions.length) {
            showQuestion();
            byId("question").focus();
            return;
        }
        byId("meter").value = answers.length;
        byId("progress").textContent = "All answers selected";
        byId("prompt").textContent = "Ready to save your score?";
        byId("choices").replaceChildren();
        next.textContent = "Save score";
    }
    if (!handle) {
        note.textContent = "Log in to save your score.";
        openAuth("login");
        return;
    }
    next.disabled = true;
    try {
        const result = await (await request("/api/attempts", {answers})).json();
        answers.length = 0;
        showQuestion();
        note.textContent = "You scored " + result.score + " out of " + result.total + ".";
        await showRanking();
    } catch (error) {
        note.textContent = error.message;
        next.disabled = false;
    }
};

try {
    const profile = await (await request("/api/me")).json();
    setAccount(profile.handle);
    questions = await (await request("/api/questions")).json();
    byId("meter").max = questions.length;
    showQuestion();
    await showRanking();
} catch (error) {
    note.textContent = error.message;
}
