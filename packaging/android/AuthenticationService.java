package @APPLICATION_ID@;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;

/** A non-sticky lease for the user-started Microsoft account exchange, never for gameplay. */
public final class AuthenticationService extends Service {
    private static final String CHANNEL = "authentication";
    private static final Object STATE = new Object();
    private static final Handler MAIN = new Handler(Looper.getMainLooper());
    private static boolean requested;
    private static AuthenticationService current;
    private Notification notification;

    public static void setActive(Context context, boolean active) {
        Context application = context.getApplicationContext();
        synchronized (STATE) { requested = active; }
        // Native startup may occupy the main thread. Begin the Android deadline
        // only once it can also deliver onCreate and promote this service.
        MAIN.post(() -> {
            synchronized (STATE) {
                if (active) {
                    if (!requested) return;
                    try {
                        application.startForegroundService(new Intent(application, AuthenticationService.class));
                    } catch (RuntimeException error) {
                        Log.e("Cinnabar", "Could not start the Microsoft sign-in foreground service", error);
                    }
                } else if (!requested && current != null) {
                    // A pending service must promote before stopping, even if
                    // cached authentication completed before onCreate ran.
                    current.stopSelf();
                }
            }
        });
    }

    @Override public void onCreate() {
        super.onCreate();
        NotificationManager manager = getSystemService(NotificationManager.class);
        manager.createNotificationChannel(new NotificationChannel(CHANNEL, "Microsoft sign-in",
                NotificationManager.IMPORTANCE_LOW));
        Intent resume = new Intent(this, @ACTIVITY@.class);
        PendingIntent content = PendingIntent.getActivity(this, 0, resume,
                PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
        notification = new Notification.Builder(this, CHANNEL)
                .setSmallIcon(getApplicationInfo().icon)
                .setContentTitle("@PRODUCT_NAME@ sign-in")
                .setContentText("Complete Microsoft sign-in in your browser")
                .setContentIntent(content).setOngoing(true).build();
        synchronized (STATE) {
            promote();
            current = this;
            if (!requested) stopSelf();
        }
    }

    private void promote() {
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(1, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC);
        } else {
            startForeground(1, notification);
        }
    }

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        synchronized (STATE) {
            // A new request may reach an instance that was already stopping.
            promote();
            current = this;
            if (!requested) stopSelfResult(startId);
        }
        return START_NOT_STICKY;
    }

    @Override public IBinder onBind(Intent intent) { return null; }

    @Override public void onTaskRemoved(Intent intent) { stopLease(); }

    @Override public void onTimeout(int startId, int type) { stopLease(); }

    private void stopLease() {
        synchronized (STATE) {
            if (current == this) requested = false;
            stopSelf();
        }
    }

    @Override public void onDestroy() {
        synchronized (STATE) {
            if (current == this) current = null;
        }
        stopForeground(STOP_FOREGROUND_REMOVE);
        super.onDestroy();
    }
}
