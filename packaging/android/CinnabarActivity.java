package @APPLICATION_ID@;

import android.app.NativeActivity;
import android.content.Context;
import android.text.InputType;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;
import java.util.concurrent.ArrayBlockingQueue;

/** NativeActivity hosts the client; all game UI is rendered by the JSON-UI engine. */
public final class CinnabarActivity extends NativeActivity {
    private final ArrayBlockingQueue<String> textInput = new ArrayBlockingQueue<>(128);
    private View inputView;
    private boolean numericInput;

    @Override protected void onDestroy() {
        // Bevy owns a process-wide AndroidApp OnceLock. Finish native teardown
        // before ending this client process so a later launch gets a fresh app.
        super.onDestroy();
        setAuthenticationActive(false);
        android.os.Process.killProcess(android.os.Process.myPid());
    }

    /** Called before launching the browser; the Go auth reader ends this lease on EOF. */
    public void setAuthenticationActive(boolean active) {
        AuthenticationService.setActive(this, active);
    }

    public void showFailure(String title, String message) {
        runOnUiThread(() -> new android.app.AlertDialog.Builder(this)
                .setTitle(title).setMessage(message).setCancelable(false)
                .setPositiveButton("Close", (dialog, which) -> finish()).show());
    }

    /** A transparent input connection supplies IME commits; JSON UI draws the editor. */
    public void setTextInputEnabled(boolean enabled, boolean numeric) {
        runOnUiThread(() -> {
            InputMethodManager manager = (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
            if (inputView == null && enabled) {
                inputView = new View(this) {
                    @Override public boolean onCheckIsTextEditor() { return true; }
                    @Override public InputConnection onCreateInputConnection(EditorInfo info) {
                        info.inputType = numericInput ? InputType.TYPE_CLASS_NUMBER
                                : InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
                        info.imeOptions = EditorInfo.IME_ACTION_DONE | EditorInfo.IME_FLAG_NO_EXTRACT_UI
                                | EditorInfo.IME_FLAG_NO_FULLSCREEN;
                        return new BaseInputConnection(this, false) {
                            private String composing = "";
                            @Override public boolean setComposingText(CharSequence text, int cursor) {
                                composing = text.toString();
                                return true;
                            }
                            @Override public boolean commitText(CharSequence text, int cursor) {
                                composing = "";
                                return enqueueText(text.toString());
                            }
                            @Override public boolean finishComposingText() {
                                String text = composing;
                                composing = "";
                                return enqueueText(text);
                            }
                            @Override public boolean deleteSurroundingText(int before, int after) {
                                composing = "";
                                return enqueueText(repeatControl('\b', before) + repeatControl('\u007f', after));
                            }
                            @Override public boolean deleteSurroundingTextInCodePoints(int before, int after) {
                                return deleteSurroundingText(before, after);
                            }
                            @Override public boolean performEditorAction(int action) {
                                finishComposingText();
                                return enqueueText("\n");
                            }
                            @Override public boolean sendKeyEvent(KeyEvent event) {
                                return routeKey(event);
                            }
                            @Override public CharSequence getTextBeforeCursor(int length, int flags) { return ""; }
                            @Override public CharSequence getTextAfterCursor(int length, int flags) { return ""; }
                            @Override public CharSequence getSelectedText(int flags) { return ""; }
                        };
                    }
                    @Override public boolean dispatchKeyEvent(KeyEvent event) {
                        return routeKey(event) || super.dispatchKeyEvent(event);
                    }
                };
                inputView.setAlpha(0);
                inputView.setFocusable(true);
                inputView.setFocusableInTouchMode(true);
                addContentView(inputView, new ViewGroup.LayoutParams(1, 1));
            }
            numericInput = numeric;
            if (enabled) {
                inputView.requestFocus();
                manager.restartInput(inputView);
                manager.showSoftInput(inputView, InputMethodManager.SHOW_IMPLICIT);
            } else if (inputView != null) {
                manager.hideSoftInputFromWindow(inputView.getWindowToken(), 0);
                inputView.clearFocus();
                textInput.clear();
            }
        });
    }

    /** Returns one ordered commit, using standard text controls for editing keys. */
    public String pollTextInput() { return textInput.poll(); }

    private boolean enqueueText(String text) {
        return text.isEmpty() || (text.length() <= 65536 && textInput.offer(text));
    }

    private static String repeatControl(char control, int count) {
        StringBuilder text = new StringBuilder();
        for (int i = 0; i < Math.min(Math.max(count, 0), 256); i++) text.append(control);
        return text.toString();
    }

    private boolean routeKey(KeyEvent event) {
        if (event.getKeyCode() == KeyEvent.KEYCODE_BACK) return false;
        if (event.getAction() != KeyEvent.ACTION_DOWN) return true;
        switch (event.getKeyCode()) {
            case KeyEvent.KEYCODE_DEL: return enqueueText("\b");
            case KeyEvent.KEYCODE_FORWARD_DEL: return enqueueText("\u007f");
            case KeyEvent.KEYCODE_ENTER: return enqueueText("\n");
            default:
                int character = event.getUnicodeChar();
                return character != 0 && enqueueText(new String(Character.toChars(character)));
        }
    }

    public void openExternalUrl(String url) {
        runOnUiThread(() -> {
            try {
                startActivity(new android.content.Intent(android.content.Intent.ACTION_VIEW,
                        android.net.Uri.parse(url)));
            } catch (android.content.ActivityNotFoundException error) {
                android.widget.Toast.makeText(this, "No app can open this link",
                        android.widget.Toast.LENGTH_SHORT).show();
            }
        });
    }
}
