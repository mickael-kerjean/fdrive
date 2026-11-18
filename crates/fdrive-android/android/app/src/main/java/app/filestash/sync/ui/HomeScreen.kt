package app.filestash.sync.ui

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.provider.DocumentsContract
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.filestash.sync.Native
import app.filestash.sync.R
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@Composable
fun HomeScreen(onLoggedOut: () -> Unit) {
    val context = LocalContext.current
    var connected by remember { mutableStateOf<Boolean?>(null) }
    var checkNow by remember { mutableIntStateOf(0) }
    val account = remember { accountLabel(context) }
    var offline by remember { mutableStateOf(false) }
    var meter by remember { mutableStateOf(emptyList<Float>()) }
    var speed by remember { mutableStateOf("") }

    LaunchedEffect(Unit) {
        while (true) {
            withContext(Dispatchers.IO) {
                runCatching { Native.withReauth(context) { it.meter() to it.speed() } }.getOrNull()
            }?.let { (bars, rate) ->
                meter = bars.map { it.toFloat() }
                speed = rate
            }
            delay(1_000)
        }
    }

    LaunchedEffect(checkNow) {
        while (true) {
            connected = withContext(Dispatchers.IO) {
                runCatching { Native.withReauth(context) { it.ls("/") } }.isSuccess
            }
            delay(10_000)
        }
    }

    Box(modifier = Modifier.fillMaxSize()) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Spacer(modifier = Modifier.weight(1f))
            StatusCircle(connected, meter, speed) { if (connected == true) offline = true else checkNow++ }
            Spacer(modifier = Modifier.height(16.dp))
            Text(
                when (connected) {
                    true -> "Connected"
                    false -> "Offline"
                    null -> "Connecting…"
                },
                style = MaterialTheme.typography.titleMedium,
            )
            account?.let {
                Spacer(modifier = Modifier.height(4.dp))
                Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.outline)
            }
            Spacer(modifier = Modifier.weight(1f))
            Button(
                onClick = { openFileManager(context) },
                colors = ButtonDefaults.buttonColors(
                    containerColor = MaterialTheme.colorScheme.secondary,
                    contentColor = MaterialTheme.colorScheme.onSecondary,
                ),
                shape = MaterialTheme.shapes.extraSmall,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(56.dp),
            ) {
                Text("Browse")
            }
        }
        IconButton(
            onClick = {
                CoroutineScope(Dispatchers.IO).launch { Native.logout(context) }
                onLoggedOut()
            },
            modifier = Modifier.align(Alignment.TopEnd),
        ) {
            LogoutIcon(MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
    if (offline) {
        PinsDialog { offline = false }
    }
}

@Composable
private fun PinsDialog(onDismiss: () -> Unit) {
    val context = LocalContext.current
    var pins by remember { mutableStateOf(Native.withReauth(context) { it.pins() }) }
    var picking by remember { mutableStateOf(false) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Pins") },
        text = {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(max = 400.dp)
                    .verticalScroll(rememberScrollState()),
            ) {
                if (pins.isEmpty()) {
                    Icon(
                        painterResource(R.drawable.ic_pin_off),
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.outline,
                        modifier = Modifier
                            .padding(32.dp)
                            .size(40.dp)
                            .align(Alignment.CenterHorizontally),
                    )
                }
                pins.forEach { path ->
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            path.trimEnd('/').substringAfterLast('/'),
                            style = MaterialTheme.typography.bodyLarge,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f),
                        )
                        IconButton(onClick = { pins = Native.withReauth(context) { it.unpin(path); it.pins() } }) {
                            Text("✕", color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { picking = true }) { Text("Add") }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        },
    )
    if (picking) {
        FolderPicker(
            start = "/",
            onSelect = { dir ->
                picking = false
                pins = Native.withReauth(context) { it.pin(dir); it.pins() }
            },
            onDismiss = { picking = false },
        )
    }
}

@Composable
private fun LogoutIcon(tint: Color) {
    Canvas(modifier = Modifier.size(22.dp)) {
        val w = size.width
        val h = size.height
        val stroke = Stroke(width = w / 11f, cap = StrokeCap.Round, join = StrokeJoin.Round)
        // Door frame, open on the right.
        drawPath(
            Path().apply {
                moveTo(w * 0.52f, h * 0.15f)
                lineTo(w * 0.17f, h * 0.15f)
                lineTo(w * 0.17f, h * 0.85f)
                lineTo(w * 0.52f, h * 0.85f)
            },
            color = tint,
            style = stroke,
        )
        // Arrow leaving through the opening.
        drawLine(
            color = tint,
            start = Offset(w * 0.40f, h * 0.5f),
            end = Offset(w * 0.88f, h * 0.5f),
            strokeWidth = stroke.width,
            cap = StrokeCap.Round,
        )
        drawPath(
            Path().apply {
                moveTo(w * 0.70f, h * 0.31f)
                lineTo(w * 0.90f, h * 0.5f)
                lineTo(w * 0.70f, h * 0.69f)
            },
            color = tint,
            style = stroke,
        )
    }
}

@Composable
private fun StatusCircle(connected: Boolean?, meter: List<Float>, speed: String, onClick: () -> Unit) {
    val fill = when (connected) {
        true -> ConnectedGreen
        false -> MaterialTheme.colorScheme.error
        null -> MaterialTheme.colorScheme.surfaceVariant
    }
    val glyph = when (connected) {
        null -> MaterialTheme.colorScheme.outline
        else -> Color(0xFF24272A)
    }
    val pulse by rememberInfiniteTransition(label = "glow").animateFloat(
        initialValue = 0f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(1600, easing = FastOutSlowInEasing),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "glow",
    )
    val glow = if (connected == true) pulse else 0f
    Box(
        modifier = Modifier
            .size(168.dp)
            .clip(CircleShape)
            .background(fill.copy(alpha = 0.14f + 0.10f * glow)),
        contentAlignment = Alignment.Center,
    ) {
        Box(
            modifier = Modifier
                .size(146.dp)
                .clip(CircleShape)
                .background(fill.copy(alpha = 0.22f + 0.12f * glow)),
        )
        StatusCircleCore(fill, glyph, meter, speed, onClick)
    }
}

@Composable
private fun StatusCircleCore(fill: Color, glyph: Color, meter: List<Float>, speed: String, onClick: () -> Unit) {
    Box(
        modifier = Modifier
            .size(124.dp)
            .clip(CircleShape)
            .background(fill)
            .drawBehind {
                val bars = meter.takeLast(24)
                val max = bars.maxOrNull()?.takeIf { it > 0f } ?: return@drawBehind
                val step = size.width / 24
                bars.forEachIndexed { i, value ->
                    val height = size.height * value / max
                    drawRect(
                        color = glyph.copy(alpha = 0.12f),
                        topLeft = Offset(size.width - (bars.size - i) * step, size.height - height),
                        size = Size(step * 0.8f, height),
                    )
                }
            }
            .clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        if (speed.isNotEmpty()) {
            Text(speed, style = MaterialTheme.typography.titleMedium, color = glyph)
        } else {
            Canvas(modifier = Modifier.size(48.dp)) {
                val stroke = Stroke(width = size.width / 9, cap = StrokeCap.Round)
                drawArc(
                    color = glyph,
                    startAngle = -60f,
                    sweepAngle = 300f,
                    useCenter = false,
                    style = stroke,
                )
                drawLine(
                    color = glyph,
                    start = Offset(size.width / 2, -size.height / 12),
                    end = Offset(size.width / 2, size.height / 2.4f),
                    strokeWidth = stroke.width,
                    cap = StrokeCap.Round,
                )
            }
        }
    }
}

private fun accountLabel(context: Context): String? {
    val creds = Native.init(context).load() ?: return null
    return creds.url.substringAfter("://").substringBefore('/')
}

private fun openFileManager(context: Context) {
    val rootUri = DocumentsContract.buildRootUri(Native.AUTHORITY, Native.ROOT_ID)
    val browse = Intent(Intent.ACTION_VIEW)
        .setDataAndType(rootUri, DocumentsContract.Root.MIME_TYPE_ITEM)
    try {
        context.startActivity(browse)
    } catch (e: ActivityNotFoundException) {
        val picker = Intent(Intent.ACTION_OPEN_DOCUMENT)
            .setType("*/*")
            .putExtra(
                DocumentsContract.EXTRA_INITIAL_URI,
                DocumentsContract.buildDocumentUri(Native.AUTHORITY, "/"),
            )
        context.startActivity(picker)
    }
}
