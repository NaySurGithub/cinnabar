package @APPLICATION_ID@;

import android.app.Activity;
import android.app.AlertDialog;
import android.app.ProgressDialog;
import android.content.Intent;
import android.os.Bundle;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.util.concurrent.atomic.AtomicInteger;

/** Prepares private resource carriers before NativeActivity receives its first window event. */
public final class BootstrapActivity extends Activity {
    static { System.loadLibrary("@CLIENT_LIBRARY@"); }
    private final AtomicInteger consent = new AtomicInteger(0);
    private ProgressDialog progress;
    private native void prepareNative();
    private native void cancelNative();

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        beginSetup();
    }

    private void beginSetup() {
        if (isDestroyed() || isFinishing()) return;
        consent.set(0);
        showProgress("Preparing @PRODUCT_NAME@");
        Thread worker = new Thread(() -> prepareNative(), "resource-setup");
        worker.setPriority(Thread.MIN_PRIORITY);
        worker.start();
    }

    public void requestConsent(String title, String body) {
        runOnUiThread(() -> {
            if (isDestroyed() || isFinishing()) { consent.set(-1); return; }
            if (progress != null) progress.dismiss();
            new AlertDialog.Builder(this).setTitle(title).setMessage(body)
                    .setPositiveButton("Accept and download", (dialog, which) -> consent.set(1))
                    .setNegativeButton("Cancel", (dialog, which) -> consent.set(-1))
                    .setOnCancelListener(dialog -> consent.set(-1)).show();
        });
    }

    public int consentState() { return consent.get(); }

    public void showProgress(String message) {
        runOnUiThread(() -> {
            if (isDestroyed() || isFinishing()) return;
            if (progress == null || !progress.isShowing()) {
                progress = new ProgressDialog(this);
                progress.setTitle("@PRODUCT_NAME@");
                progress.setIndeterminate(true);
                progress.setCancelable(true);
                progress.setOnCancelListener(dialog -> { cancelNative(); finish(); });
                progress.show();
            }
            progress.setMessage(message);
        });
    }

    /** Called on the worker; Android's AssetManager delivers only the installed APK's resources. */
    public String stageResourceArchive(String name) throws java.io.IOException {
        File output = new File(getCacheDir(), name);
        try (InputStream source = getAssets().open(name);
             FileOutputStream target = new FileOutputStream(output)) {
            byte[] buffer = new byte[65536];
            long total = 0;
            int count;
            while ((count = source.read(buffer)) != -1) {
                total += count;
                if (total > @ARCHIVE_BYTES@L) throw new java.io.IOException("APK resources exceed their size limit");
                target.write(buffer, 0, count);
            }
            target.getFD().sync();
        }
        return output.getAbsolutePath();
    }

    public void setupComplete(boolean ready, String error) {
        runOnUiThread(() -> {
            if (isDestroyed() || isFinishing()) return;
            if (progress != null) progress.dismiss();
            if (ready) {
                startActivity(new Intent(this, @ACTIVITY@.class));
                finish();
            } else if (error.isEmpty()) {
                finish();
            } else {
                new AlertDialog.Builder(this).setTitle("@PRODUCT_NAME@ setup failed")
                        .setMessage(error).setPositiveButton("Retry", (dialog, which) -> beginSetup())
                        .setNegativeButton("Close", (dialog, which) -> finish())
                        .setOnCancelListener(dialog -> finish()).show();
            }
        });
    }

    @Override protected void onDestroy() {
        cancelNative();
        if (progress != null) progress.dismiss();
        super.onDestroy();
    }
}
