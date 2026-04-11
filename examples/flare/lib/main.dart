import 'package:flare_im/app.dart';
import 'package:flare_im/infrastructure/media/composer_pack_assets.dart';
import 'package:flare_im/infrastructure/media/emoji_pack_i18n.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await Future.wait([
    EmojiPackI18n.ensureLoaded(),
    ComposerPackAssets.ensureLoaded(),
  ]);
  runApp(
    const ProviderScope(
      child: FlareImApp(),
    ),
  );
}
