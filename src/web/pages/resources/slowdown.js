const SLOWDOWN_TIMEOUT = 5 * 1000;

document.querySelectorAll(".slowdown").forEach((element) => element.setAttribute("disabled", ""));

setTimeout(() => {
    document.querySelectorAll(".slowdown").forEach((element) => element.removeAttribute("disabled"));
}, SLOWDOWN_TIMEOUT);
