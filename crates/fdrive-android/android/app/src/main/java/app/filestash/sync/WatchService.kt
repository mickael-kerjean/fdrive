package app.filestash.sync

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.IBinder
import android.os.Looper

class WatchService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val channel = NotificationChannel("watch", "Live updates", NotificationManager.IMPORTANCE_MIN)
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
        startForeground(1, Notification.Builder(this, "watch").setSmallIcon(android.R.drawable.stat_notify_sync).build())
        return START_NOT_STICKY
    }

    override fun onTimeout(startId: Int, fgsType: Int) = stopSelf()

    companion object {
        private val handler = Handler(Looper.getMainLooper())

        fun keepAlive(context: Context) {
            val intent = Intent(context, WatchService::class.java)
            runCatching { context.startForegroundService(intent) }
            handler.removeCallbacksAndMessages(null)
            handler.postDelayed({ context.stopService(intent) }, 5 * 60_000L)
        }
    }
}
