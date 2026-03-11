(function () {
    if (typeof window === "undefined" || !window.htmx) return;

    var api = null;

    function getAttribute(el, name) {
        if (!el || !el.getAttribute) return null;
        return el.getAttribute(name) || el.getAttribute("data-" + name);
    }

    function hasAttribute(el, name) {
        if (!el || !el.hasAttribute) return false;
        return el.hasAttribute(name) || el.hasAttribute("data-" + name);
    }

    function collectSwapTargets(root) {
        var targets = [];
        if (hasAttribute(root, "sse-swap")) {
            targets.push(root);
        }
        if (!root || !root.querySelectorAll) return targets;
        var nodes = root.querySelectorAll("[sse-swap], [data-sse-swap]");
        for (var i = 0; i < nodes.length; i++) {
            if (nodes[i] !== root) {
                targets.push(nodes[i]);
            }
        }
        return targets;
    }

    function toFragment(data) {
        var template = document.createElement("template");
        template.innerHTML = data;
        return template.content;
    }

    function applyOobSwaps(fragment) {
        if (!fragment || !fragment.querySelectorAll) return;
        var oob = fragment.querySelectorAll(
            "[hx-swap-oob], [data-hx-swap-oob]",
        );
        for (var i = 0; i < oob.length; i++) {
            var el = oob[i];
            var spec =
                el.getAttribute("hx-swap-oob") ||
                el.getAttribute("data-hx-swap-oob") ||
                "true";
            var id = el.getAttribute("id");
            if (id) {
                var target = document.getElementById(id);
                if (target) {
                    if (spec === "delete") {
                        target.remove();
                    } else if (spec === "true" || spec === "outerHTML") {
                        target.replaceWith(el.cloneNode(true));
                    }
                }
            }
            if (el.parentNode) {
                el.parentNode.removeChild(el);
            }
        }
    }

    function swapInto(target, data) {
        if (!target) return;
        var fragment = toFragment(data);
        applyOobSwaps(fragment);
        var swapSpec = api ? api.getSwapSpecification(target) : null;
        var style = (swapSpec && swapSpec.swapStyle) || "innerHTML";
        switch (style) {
            case "beforeend":
                target.appendChild(fragment);
                break;
            case "afterbegin":
                target.insertBefore(fragment, target.firstChild);
                break;
            case "beforebegin":
                if (target.parentNode) {
                    target.parentNode.insertBefore(fragment, target);
                }
                break;
            case "afterend":
                if (target.parentNode) {
                    target.parentNode.insertBefore(fragment, target.nextSibling);
                }
                break;
            case "outerHTML":
                if (target.parentNode) {
                    var parent = target.parentNode;
                    parent.insertBefore(fragment, target);
                    parent.removeChild(target);
                }
                break;
            case "innerHTML":
            default:
                target.innerHTML = "";
                target.appendChild(fragment);
                break;
        }
        var processTarget = target.parentElement || target;
        if (window.htmx && typeof window.htmx.process === "function") {
            window.htmx.process(processTarget);
        }
    }

    function connectSource(root) {
        var url = getAttribute(root, "sse-connect");
        if (!url || !api) return;
        var internal = api.getInternalData(root);
        if (internal.sseEventSource) return;

        var source = window.htmx.createEventSource(url);
        internal.sseEventSource = source;
        internal.sseListenerInfos = [];

        var targets = collectSwapTargets(root);
        for (var i = 0; i < targets.length; i++) {
            (function (target) {
                var eventName = getAttribute(target, "sse-swap") || "message";
                var listener = function (evt) {
                    swapInto(target, evt.data || "");
                };
                source.addEventListener(eventName, listener);
                internal.sseListenerInfos.push({
                    event: eventName,
                    listener: listener,
                });
            })(targets[i]);
        }

        source.onerror = function (evt) {
            if (window.htmx && typeof window.htmx.trigger === "function") {
                window.htmx.trigger(root, "htmx:sseError", {
                    error: evt,
                    source: source,
                });
            }
        };
    }

    function disconnectSource(root) {
        if (!api) return;
        var internal = api.getInternalData(root);
        if (!internal || !internal.sseEventSource) return;
        if (internal.sseListenerInfos) {
            for (var i = 0; i < internal.sseListenerInfos.length; i++) {
                var info = internal.sseListenerInfos[i];
                internal.sseEventSource.removeEventListener(
                    info.event,
                    info.listener,
                );
            }
        }
        internal.sseEventSource.close();
        delete internal.sseEventSource;
        delete internal.sseListenerInfos;
    }

    window.htmx.defineExtension("sse", {
        init: function (apiRef) {
            api = apiRef;
        },
        onEvent: function (name, evt) {
            if (!evt || !evt.detail) return;
            if (name === "htmx:afterProcessNode") {
                connectSource(evt.detail.elt);
            }
            if (name === "htmx:beforeCleanupElement") {
                disconnectSource(evt.detail.elt);
            }
            return true;
        },
    });
})();
