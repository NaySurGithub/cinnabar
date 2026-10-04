package @APPLICATION_ID@;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;

/** A non-sticky lease for the user-started Microsoft account exchange, never for gameplay. */
public final class AuthenticationService extends Service {
    private static final String CHANNEL = "authentication";

    @Override public void onCreate() {
        super.onCreate();
        NotificationManager manager = getSystemService(NotificationManager.class);
        manager.createNotificationChannel(new NotificationChannel(CHANNEL, "Microsoft sign-in",
                NotificationManager.IMPORTANCE_LOW));
        Intent resume = new Intent(this, @ACTIVITY@.class);
        PendingIntent content = PendingIntent.getActivity(this, 0, resume,
                PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
        Notification notification = new Notification.Builder(this, CHANNEL)
                .setSmallIcon(getApplicationInfo().icon)
                .setContentTitle("@PRODUCT_NAME@ sign-in")
                .setContentText("Complete Microsoft sign-in in your browser")
                .setContentIntent(content).setOngoing(true).build();
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(1, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC);
        } else {
            startForeground(1, notification);
        }
    }

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        return START_NOT_STICKY;
    }

    @Override public IBinder onBind(Intent intent) { return null; }

    @Override public void onTaskRemoved(Intent intent) { stopSelf(); }

    @Override public void onTimeout(int startId, int type) { stopSelf(); }

    @Override public void onDestroy() {
        stopForeground(STOP_FOREGROUND_REMOVE);
        super.onDestroy();
    }
}
