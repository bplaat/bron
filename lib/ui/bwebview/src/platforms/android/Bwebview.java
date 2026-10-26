/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

package nl.bplaat.bwebview;

import java.io.ByteArrayInputStream;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.List;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.regex.Pattern;

import android.app.Activity;
import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.PixelFormat;
import android.net.Uri;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.view.ContextThemeWrapper;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.WindowManager;
import android.webkit.JavascriptInterface;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;

/**
 * A WebView in a panel window over the native activity window, controlled from the Rust main
 * thread. Every method can be called from any thread and runs on the UI thread in call order.
 * Custom protocols are served from https://{scheme}.appassets.androidplatform.net/.
 */
public final class Bwebview {
    private static final String HOST_SUFFIX = ".appassets.androidplatform.net";
    private static final int MAX_REDIRECTS = 10;
    private static final Pattern HEAD_TAG = Pattern.compile("<head(\\s[^>]*)?>", Pattern.CASE_INSENSITIVE);

    private final Handler handler = new Handler(Looper.getMainLooper());
    private final long id;
    private final List<String> schemes;
    private final List<String> startScripts = new CopyOnWriteArrayList<>();
    private final List<String> loadedScripts = new ArrayList<>();
    private volatile String injectedUrl;
    private Activity activity;
    private Panel panel;
    private WebView webView;

    /** A transparent backgroundColor keeps the WebView default background. */
    public Bwebview(Activity activity, long id, int backgroundColor, String userAgent, String[] schemes) {
        this.id = id;
        this.schemes = Arrays.asList(schemes);
        handler.post(() -> create(activity, backgroundColor, userAgent));
    }

    @SuppressWarnings("deprecation")
    private void create(Activity activity, int backgroundColor, String userAgent) {
        this.activity = activity;
        // The WebView reports prefers-color-scheme from its theme, a day night theme follows the
        // system or app night mode also when the activity uses the default light theme.
        var context = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q
            ? new ContextThemeWrapper(activity, android.R.style.Theme_DeviceDefault_DayNight)
            : activity;
        webView = new WebView(context);
        var settings = webView.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setUserAgentString(settings.getUserAgentString() + " " + userAgent);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            settings.setAlgorithmicDarkeningAllowed(false);
        } else {
            settings.setForceDark(WebSettings.FORCE_DARK_OFF);
        }
        if (backgroundColor != 0) {
            webView.setBackgroundColor(backgroundColor);
        }
        webView.addJavascriptInterface(new Bridge(), "bwebview");
        webView.setWebViewClient(new Client());
        webView.setWebChromeClient(new ChromeClient());
        panel = new Panel(activity);
        panel.addView(webView);
        // A panel window needs the token of the activity window, which exists once attached.
        activity.getWindow().getDecorView().post(this::show);
    }

    @SuppressWarnings("deprecation")
    private void show() {
        if (webView == null) {
            return;
        }
        var params = new WindowManager.LayoutParams(WindowManager.LayoutParams.TYPE_APPLICATION_PANEL,
            WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN | WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS,
            PixelFormat.TRANSLUCENT);
        params.softInputMode = WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE;
        params.layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            params.setFitInsetsTypes(0);
        }
        activity.getWindowManager().addView(panel, params);
    }

    public void loadUrl(String url) {
        handler.post(() -> webView.loadUrl(url));
    }

    public void loadHtml(String html) {
        handler.post(() -> webView.loadDataWithBaseURL(null, html, "text/html", "utf-8", null));
    }

    public void evaluateScript(String script) {
        handler.post(() -> webView.evaluateJavascript(script, null));
    }

    public void addUserScript(String script, boolean documentStart) {
        handler.post(() -> (documentStart ? startScripts : loadedScripts).add(script));
    }

    public void setBackgroundColor(int color) {
        handler.post(() -> webView.setBackgroundColor(color));
    }

    /** Remove and destroy the WebView. */
    public void close() {
        handler.post(() -> {
            if (panel.isAttachedToWindow()) {
                activity.getWindowManager().removeViewImmediate(panel);
            }
            webView.destroy();
            webView = null;
        });
    }

    // MARK: Panel
    // Fills the activity window, pads the WebView by the system bars, display cutout, and
    // keyboard, and handles the back key, which the panel window receives while focused, by
    // navigating back in the history or else forwarding it like the main window back gesture.
    private final class Panel extends FrameLayout {
        Panel(Context context) {
            super(context);
        }

        @Override
        @SuppressWarnings("deprecation")
        public WindowInsets onApplyWindowInsets(WindowInsets insets) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                var bars = insets.getInsets(
                    WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout() | WindowInsets.Type.ime());
                setPadding(bars.left, bars.top, bars.right, bars.bottom);
                // The panel already applied these insets; WebView must not apply them again.
                return WindowInsets.CONSUMED;
            } else {
                setPadding(insets.getSystemWindowInsetLeft(), insets.getSystemWindowInsetTop(),
                    insets.getSystemWindowInsetRight(), insets.getSystemWindowInsetBottom());
                var remaining = insets.consumeSystemWindowInsets().consumeStableInsets();
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                    remaining = remaining.consumeDisplayCutout();
                }
                return remaining;
            }
        }

        @Override
        @SuppressWarnings("deprecation")
        protected void onAttachedToWindow() {
            super.onAttachedToWindow();
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
                setSystemUiVisibility(View.SYSTEM_UI_FLAG_LAYOUT_STABLE | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION);
            }
        }

        @Override
        public boolean dispatchKeyEvent(KeyEvent event) {
            if (event.getKeyCode() == KeyEvent.KEYCODE_BACK) {
                if (event.getAction() == KeyEvent.ACTION_UP) {
                    if (webView.canGoBack()) {
                        webView.goBack();
                    } else {
                        nativeBack();
                    }
                }
                return true;
            }
            return super.dispatchKeyEvent(event);
        }

        @Override
        public void addView(View child) {
            super.addView(
                child, new LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        }
    }

    // MARK: Clients
    private final class Client extends WebViewClient {
        @Override
        public void onPageStarted(WebView view, String url, Bitmap favicon) {
            // Pages served by a custom protocol already have the scripts in their HTML.
            if (!url.equals(injectedUrl)) {
                for (var script : startScripts) {
                    view.evaluateJavascript(script, null);
                }
            }
            nativeEvent(id, "start", url);
        }

        @Override
        public void onPageFinished(WebView view, String url) {
            for (var script : loadedScripts) {
                view.evaluateJavascript(script, null);
            }
            nativeEvent(id, "finish", url);
        }

        // Runs on a WebView background thread.
        @Override
        public WebResourceResponse shouldInterceptRequest(WebView view, WebResourceRequest request) {
            var url = request.getUrl();
            if (!isCustomProtocol(url)) {
                return null;
            }
            var mainFrameUrl = request.isForMainFrame() ? url.toString() : null;
            // WebResourceResponse can't express redirects, so follow them to our own protocols here.
            for (var redirects = 0; redirects <= MAX_REDIRECTS; redirects++) {
                var arguments = new ArrayList<String>();
                arguments.add(request.getMethod());
                arguments.add(url.toString());
                for (var header : request.getRequestHeaders().entrySet()) {
                    arguments.add(header.getKey());
                    arguments.add(header.getValue());
                }
                var bytes = nativeRequest(id, arguments.toArray(new String[0]));
                if (bytes == null) {
                    return null;
                }
                var response = parseResponse(bytes);
                if (response.status < 300 || response.status >= 400) {
                    return toWebResourceResponse(response, mainFrameUrl);
                }
                var location = header(response.headers, "Location");
                if (location == null) {
                    break;
                }
                try {
                    url = Uri.parse(URI.create(url.toString()).resolve(location).toString());
                } catch (IllegalArgumentException exception) {
                    break;
                }
                if (!isCustomProtocol(url)) {
                    break;
                }
            }
            return new WebResourceResponse("text/plain", "utf-8", 502, "Bad Gateway", new HashMap<>(),
                new ByteArrayInputStream("Unsupported redirect".getBytes(StandardCharsets.UTF_8)));
        }
    }

    private final class ChromeClient extends WebChromeClient {
        @Override
        public void onReceivedTitle(WebView view, String title) {
            nativeEvent(id, "title", title);
        }
    }

    private final class Bridge {
        @JavascriptInterface
        public void postMessage(String message) {
            nativeEvent(id, "message", message);
        }
    }

    private boolean isCustomProtocol(Uri url) {
        var host = url.getHost();
        return "https".equals(url.getScheme()) && host != null && host.endsWith(HOST_SUFFIX)
            && schemes.contains(host.substring(0, host.length() - HOST_SUFFIX.length()));
    }

    // MARK: Responses
    private record Response(int status, String reason, HashMap<String, String> headers, byte[] body) {}

    private static String header(HashMap<String, String> headers, String name) {
        for (var header : headers.entrySet()) {
            if (header.getKey().equalsIgnoreCase(name)) {
                return header.getValue();
            }
        }
        return null;
    }

    // The response is a "200 OK" status line, "Name: value" header lines, an empty line, and the body.
    private static Response parseResponse(byte[] response) {
        var headers = new HashMap<String, String>();
        var position = 0;
        var status = "";
        while (true) {
            var end = position;
            while (response[end] != '\n') {
                end++;
            }
            var line = new String(response, position, end - position, StandardCharsets.UTF_8);
            position = end + 1;
            if (line.isEmpty()) {
                break;
            }
            if (status.isEmpty()) {
                status = line;
            } else {
                var colon = line.indexOf(':');
                headers.put(line.substring(0, colon), line.substring(colon + 1).trim());
            }
        }
        var body = new byte[response.length - position];
        System.arraycopy(response, position, body, 0, body.length);
        var space = status.indexOf(' ');
        return new Response(Integer.parseInt(status.substring(0, space)), status.substring(space + 1), headers, body);
    }

    private WebResourceResponse toWebResourceResponse(Response response, String mainFrameUrl) {
        var contentType = header(response.headers, "Content-Type");
        var mimeType = (contentType != null ? contentType : "application/octet-stream").split(";")[0].trim();
        var body = response.body;
        if (mainFrameUrl != null && mimeType.equals("text/html") && !startScripts.isEmpty()) {
            body = injectScripts(body);
            injectedUrl = mainFrameUrl;
        }
        return new WebResourceResponse(
            mimeType, "utf-8", response.status, response.reason, response.headers, new ByteArrayInputStream(body));
    }

    // Insert the document start scripts after <head>, or at the start of the document.
    private byte[] injectScripts(byte[] body) {
        var html = new String(body, StandardCharsets.UTF_8);
        var scripts = new StringBuilder("<script>");
        for (var script : startScripts) {
            scripts.append(script).append('\n');
        }
        scripts.append("</script>");
        var head = HEAD_TAG.matcher(html);
        var insert = head.find() ? head.end() : 0;
        return (html.substring(0, insert) + scripts + html.substring(insert)).getBytes(StandardCharsets.UTF_8);
    }

    // MARK: Native methods
    private static native void nativeEvent(long id, String kind, String value);

    private static native byte[] nativeRequest(long id, String[] request);

    private static native void nativeBack();
}
