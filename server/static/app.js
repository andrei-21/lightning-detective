(function () {
    var form = document.querySelector("form[hx-post]");
    var privacyNote = document.getElementById("privacyNote");
    var dismissPrivacyNote = document.getElementById("dismissPrivacyNote");
    var PRIVACY_NOTE_COOKIE = "privacyNoteDismissed";
    var PRIVACY_NOTE_MAX_AGE = 60 * 60 * 24 * 180; // roughly 6 months

    function hasDismissedPrivacyNote() {
        if (!document.cookie) return false;
        var cookies = document.cookie.split(";");
        for (var i = 0; i < cookies.length; i++) {
            var cookie = cookies[i].trim();
            if (cookie.indexOf(PRIVACY_NOTE_COOKIE + "=") === 0) {
                return cookie.substring(PRIVACY_NOTE_COOKIE.length + 1) === "true";
            }
        }
        return false;
    }

    function rememberPrivacyNoteDismissal() {
        try {
            document.cookie =
                PRIVACY_NOTE_COOKIE +
                "=true; path=/; max-age=" +
                PRIVACY_NOTE_MAX_AGE;
        } catch (err) {
            console.error("Unable to persist privacy note dismissal", err);
        }
    }

    if (privacyNote && hasDismissedPrivacyNote()) {
        privacyNote.remove();
        privacyNote = null;
    }

    if (privacyNote && dismissPrivacyNote) {
        dismissPrivacyNote.addEventListener("click", function (event) {
            event.preventDefault();
            rememberPrivacyNoteDismissal();
            if (privacyNote) {
                privacyNote.remove();
                privacyNote = null;
            }
        });
    }

    if (!form) return;
    var textarea = document.getElementById("text");
    var submitBtn = form.querySelector('button[type="submit"]');
    var clearBtn = document.getElementById("clearBtn");
    var result = document.getElementById("result");

    function updateInputState() {
        var hasValue = textarea && textarea.value.trim().length > 0;
        if (clearBtn) {
            clearBtn.disabled = !hasValue;
        }
        if (submitBtn && !submitBtn.hasAttribute("aria-busy")) {
            submitBtn.disabled = !hasValue;
        }
    }

    function updateAddressBar(requestValue) {
        if (!window.history || !window.URL) return;
        try {
            var current = new URL(window.location.href);
            if (requestValue && requestValue.length) {
                current.searchParams.set("r", requestValue);
            } else {
                current.searchParams.delete("r");
            }
            history.replaceState(null, "", current.toString());
        } catch (err) {
            console.error("Unable to update address bar", err);
        }
    }

    function pushCurrentPageToHistory() {
        if (!window.history || typeof history.pushState !== "function") return;
        try {
            history.pushState(null, "", window.location.href);
        } catch (err) {
            console.error("Unable to push current page to history", err);
        }
    }

    async function copyToClipboard(text) {
        if (!text) return false;
        if (navigator.clipboard && navigator.clipboard.writeText) {
            try {
                await navigator.clipboard.writeText(text);
                return true;
            } catch (err) {
                console.error("Clipboard copy failed", err);
            }
        }
        var helper = document.createElement("textarea");
        helper.value = text;
        helper.setAttribute("readonly", "");
        helper.style.position = "absolute";
        helper.style.left = "-9999px";
        document.body.appendChild(helper);
        helper.select();
        try {
            var copied = document.execCommand && document.execCommand("copy");
            if (!copied) {
                console.error("Clipboard fallback did not report success");
            }
            return !!copied;
        } catch (err) {
            console.error("Clipboard fallback failed", err);
            return false;
        } finally {
            document.body.removeChild(helper);
        }
    }

    function getInvestigatedRequest() {
        try {
            var current = new URL(window.location.href);
            return (current.searchParams.get("r") || "").trim();
        } catch (err) {
            console.error("Unable to read request from URL", err);
            return "";
        }
    }

    function wireQrButton() {
        if (!result) return;
        var toggleBtn = result.querySelector("button.show-qr");
        var qrPanel = result.querySelector(".qr-panel");
        var qrWrap = result.querySelector(".qr-canvas-wrap");
        var qrDownloadBtn = result.querySelector("button.qr-download");

        if (!toggleBtn || !qrPanel || !qrWrap) return;
        if (toggleBtn.dataset.qrWired === "true") return;
        toggleBtn.dataset.qrWired = "true";

        var request = getInvestigatedRequest();
        if (!request) {
            toggleBtn.disabled = true;
            return;
        }

        function getRenderedQrNode() {
            return qrWrap.querySelector("canvas") || qrWrap.querySelector("img");
        }

        function ensureQrRendered() {
            if (getRenderedQrNode()) return true;
            if (typeof QRCode === "undefined") {
                console.error("QRCode library is not available");
                qrWrap.textContent = "QR generator unavailable";
                return false;
            }
            qrWrap.innerHTML = "";
            try {
                new QRCode(qrWrap, {
                    // Let the library pick an appropriate QR version for long/unicode payloads.
                    typeNumber: 0,
                    text: request,
                    width: 280,
                    height: 280,
                    correctLevel: QRCode.CorrectLevel.M,
                });
                return !!getRenderedQrNode();
            } catch (err) {
                console.error("Failed to render QR code", err);
                qrWrap.textContent = "Unable to generate QR for this input";
                return false;
            }
        }

        toggleBtn.addEventListener("click", function () {
            var isHidden = qrPanel.hasAttribute("hidden");
            if (isHidden) {
                if (!ensureQrRendered()) return;
                qrPanel.removeAttribute("hidden");
                toggleBtn.textContent = "Hide QR";
                return;
            }
            qrPanel.setAttribute("hidden", "");
            toggleBtn.textContent = "Show QR";
        });

        if (qrDownloadBtn) {
            qrDownloadBtn.addEventListener("click", function () {
                if (!ensureQrRendered()) return;
                var qrNode = getRenderedQrNode();
                if (!qrNode) return;

                var dataUrl = "";
                if (qrNode.tagName.toLowerCase() === "canvas" && qrNode.toDataURL) {
                    dataUrl = qrNode.toDataURL("image/png");
                } else if (qrNode.tagName.toLowerCase() === "img") {
                    dataUrl = qrNode.src || "";
                }
                if (!dataUrl) return;

                var link = document.createElement("a");
                link.href = dataUrl;
                link.download = "payment-request-qr.png";
                document.body.appendChild(link);
                link.click();
                document.body.removeChild(link);
            });
        }
    }

    function wireShareButton() {
        if (!result) return;
        var btn = result.querySelector("button.share");
        if (!btn || btn.dataset.shareWired === "true") return;
        btn.dataset.shareWired = "true";
        btn.addEventListener("click", async function () {
            if (typeof window === "undefined" || typeof navigator === "undefined") return;
            var originalLabel = btn.textContent;
            var shareUrl = window.location.href;
            var shareData = {
                title: document.title || "Lightning Detective",
                text: "Investigate this payment instruction.",
                url: shareUrl,
            };
            async function fallbackCopy() {
                var copied = await copyToClipboard(shareUrl);
                if (copied) {
                    btn.textContent = "Copied!";
                    setTimeout(function () {
                        btn.textContent = originalLabel;
                    }, 2000);
                } else {
                    btn.textContent = originalLabel;
                }
            }
            btn.disabled = true;
            try {
                var shareSupported = typeof navigator.share === "function";
                if (shareSupported && typeof navigator.canShare === "function") {
                    try {
                        shareSupported = navigator.canShare({ url: shareUrl });
                    } catch (err) {
                        console.error("navigator.canShare threw", err);
                        shareSupported = false;
                    }
                }
                if (shareSupported) {
                    await navigator.share(shareData);
                    btn.textContent = "Shared!";
                    setTimeout(function () {
                        btn.textContent = originalLabel;
                    }, 2000);
                    return;
                }
                await fallbackCopy();
            } catch (err) {
                if (err && err.name === "AbortError") {
                    btn.textContent = originalLabel;
                } else {
                    console.error("Web Share failed, falling back to clipboard", err);
                    await fallbackCopy();
                }
            } finally {
                btn.disabled = false;
            }
        });
    }

    function setLoading(state) {
        if (!submitBtn) return;
        if (state) {
            submitBtn.setAttribute("aria-busy", "true");
            submitBtn.setAttribute("data-original-text", submitBtn.textContent || "");
            submitBtn.textContent = "Investigating ...";
            submitBtn.disabled = true;
        } else {
            submitBtn.removeAttribute("aria-busy");
            var orig = submitBtn.getAttribute("data-original-text");
            submitBtn.textContent = orig || "Submit";
            submitBtn.removeAttribute("data-original-text");
            updateInputState();
        }
    }

    var REQUEST_FORM_SELECTOR = "form[data-request-form]";

    function getRequestForm(element) {
        if (!element || typeof element.closest !== "function") return null;
        if (element.matches && element.matches(REQUEST_FORM_SELECTOR)) {
            return element;
        }
        return element.closest(REQUEST_FORM_SELECTOR);
    }

    function setRequestState(form, loading) {
        if (!form) return;
        var button = form.querySelector("button[type=submit]");
        if (!button) return;
        if (loading) {
            button.setAttribute("aria-busy", "true");
            button.setAttribute("data-original-text", button.textContent || "");
            button.textContent = "Requesting ...";
            button.disabled = true;
        } else {
            button.removeAttribute("aria-busy");
            var orig = button.getAttribute("data-original-text");
            button.textContent = orig || "Request";
            button.removeAttribute("data-original-text");
            button.disabled = false;
        }
    }

    function isMainFormEvent(event) {
        if (!event || !form) return false;
        var src = event.detail && event.detail.elt;
        if (!src) return false;
        var formElement = typeof src.closest === "function" ? src.closest("form") : null;
        return formElement === form;
    }

    function handleBeforeRequest(event) {
        if (isMainFormEvent(event)) {
            pushCurrentPageToHistory();
            setLoading(true);
            if (textarea) {
                updateAddressBar(textarea.value.trim());
            }
        }
        var requestForm = getRequestForm(event.detail && event.detail.elt);
        setRequestState(requestForm, true);
    }

    function handleAfterRequest(event) {
        if (isMainFormEvent(event)) {
            setLoading(false);
        }
        var requestForm = getRequestForm(event.detail && event.detail.elt);
        setRequestState(requestForm, false);
    }

    document.addEventListener("htmx:beforeRequest", handleBeforeRequest);
    document.addEventListener("htmx:afterRequest", handleAfterRequest);
    document.addEventListener("htmx:requestError", function (event) {
        if (isMainFormEvent(event)) {
            setLoading(false);
        }
        var requestForm = getRequestForm(event.detail && event.detail.elt);
        setRequestState(requestForm, false);
    });
    document.addEventListener("htmx:afterSwap", function (event) {
        if (!result) return;
        var target = event.detail && event.detail.target ? event.detail.target : null;
        if (target === result) {
            result.scrollIntoView({ behavior: "smooth" });
            wireShareButton();
            wireQrButton();
            return;
        }
        if (!target || !target.id) return;
        if (target.id === "lnurl-result" || target.id === "offer-result") {
            target.scrollIntoView({ behavior: "smooth" });
        }
    });

    if (textarea) {
        textarea.addEventListener("input", updateInputState);
        textarea.addEventListener("keydown", function (event) {
            if (
                event.key === "Enter" &&
                !event.shiftKey &&
                !event.altKey &&
                !event.ctrlKey &&
                !event.metaKey
            ) {
                event.preventDefault();
                if (form && typeof form.requestSubmit === "function") {
                    form.requestSubmit();
                } else if (form) {
                    form.submit();
                }
            }
        });

        function setupPasteHandler() {
            if (hasDismissedPrivacyNote()) {
                textarea.addEventListener("paste", function () {
                    setTimeout(function () {
                        if (form && typeof form.requestSubmit === "function") {
                            form.requestSubmit();
                        } else if (form) {
                            form.submit();
                        }
                    }, 0);
                });
            }
        }

        setupPasteHandler();

        if (dismissPrivacyNote) {
            dismissPrivacyNote.addEventListener(
                "click",
                function () {
                    setupPasteHandler();
                },
                { once: true },
            );
        }
    }

    if (clearBtn) {
        clearBtn.addEventListener("click", function () {
            if (textarea) {
                textarea.value = "";
                textarea.focus();
            }
            if (result) {
                result.innerHTML = "";
            }
            updateInputState();
            updateAddressBar("");
        });
    }

    wireShareButton();
    wireQrButton();
    updateInputState();
})();
