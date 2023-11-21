import 'dart:async';

import 'package:appflowy/env/env.dart';
import 'package:appflowy/generated/locale_keys.g.dart';
import 'package:appflowy/workspace/application/settings/setting_supabase_bloc.dart';
import 'package:appflowy/workspace/presentation/home/toast.dart';
import 'package:appflowy_backend/dispatch/dispatch.dart';
import 'package:appflowy_backend/protobuf/flowy-error/errors.pb.dart';
import 'package:appflowy_backend/protobuf/flowy-user/user_setting.pb.dart';
import 'package:dartz/dartz.dart' show Either;
import 'package:easy_localization/easy_localization.dart';
import 'package:flowy_infra/size.dart';
import 'package:flowy_infra_ui/flowy_infra_ui.dart';
import 'package:flowy_infra_ui/widget/error_page.dart';
import 'package:flowy_infra_ui/widget/flowy_tooltip.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_bloc/flutter_bloc.dart';

import 'setting_cloud_url.dart';

class SettingCloudView extends StatelessWidget {
  final String userId;
  const SettingCloudView({required this.userId, super.key});

  @override
  Widget build(BuildContext context) {
    return FutureBuilder<Either<CloudSettingPB, FlowyError>>(
      future: UserEventGetCloudConfig().send(),
      builder: (context, snapshot) {
        if (snapshot.data != null &&
            snapshot.connectionState == ConnectionState.done) {
          return snapshot.data!.fold(
            (config) {
              return BlocProvider(
                create: (context) => SupabaseCloudSettingBloc(
                  userId: userId,
                  config: config,
                )..add(const SupabaseCloudSettingEvent.initial()),
                child: BlocBuilder<SupabaseCloudSettingBloc,
                    SupabaseCloudSettingState>(
                  builder: (context, state) {
                    return Column(
                      children: [
                        const EnableSync(),
                        // Currently the appflowy cloud is not support end-to-end encryption.
                        if (!isAppFlowyCloudEnabled) const EnableEncrypt(),
                        if (isAppFlowyCloudEnabled)
                          AppFlowyCloudInformationWidget(
                            url: state.config.serverUrl,
                          ),
                      ],
                    );
                  },
                ),
              );
            },
            (err) {
              return FlowyErrorPage.message(err.toString(), howToFix: "");
            },
          );
        } else {
          return const Center(
            child: CircularProgressIndicator(),
          );
        }
      },
    );
  }
}

class AppFlowyCloudInformationWidget extends StatelessWidget {
  final String url;

  const AppFlowyCloudInformationWidget({required this.url, super.key});

  @override
  Widget build(BuildContext context) {
    return const Column(
      children: [
        CloudURLConfiguration(),
        // Row(
        //   children: [
        //     Expanded(
        //       // Wrap the Opacity widget with Expanded
        //       child: Opacity(
        //         opacity: 0.6,
        //         child: FlowyText(
        //           "${LocaleKeys.settings_menu_cloudURL.tr()}: $url",
        //           maxLines: null, // Allow the text to wrap
        //         ),
        //       ),
        //     ),
        //   ],
        // ),
      ],
    );
  }
}

class EnableEncrypt extends StatelessWidget {
  const EnableEncrypt({super.key});

  @override
  Widget build(BuildContext context) {
    return BlocBuilder<SupabaseCloudSettingBloc, SupabaseCloudSettingState>(
      builder: (context, state) {
        final indicator = state.loadingState.when(
          loading: () => const CircularProgressIndicator.adaptive(),
          finish: (successOrFail) => const SizedBox.shrink(),
        );

        return Column(
          children: [
            Row(
              children: [
                FlowyText.medium(LocaleKeys.settings_menu_enableEncrypt.tr()),
                const Spacer(),
                indicator,
                const HSpace(3),
                Switch(
                  onChanged: state.config.enableEncrypt
                      ? null
                      : (bool value) {
                          context.read<SupabaseCloudSettingBloc>().add(
                              SupabaseCloudSettingEvent.enableEncrypt(value));
                        },
                  value: state.config.enableEncrypt,
                ),
              ],
            ),
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisAlignment: MainAxisAlignment.start,
              children: [
                IntrinsicHeight(
                  child: Opacity(
                    opacity: 0.6,
                    child: FlowyText.medium(
                      LocaleKeys.settings_menu_enableEncryptPrompt.tr(),
                      maxLines: 13,
                    ),
                  ),
                ),
                const VSpace(6),
                SizedBox(
                  height: 40,
                  child: FlowyTooltip(
                    message: LocaleKeys.settings_menu_clickToCopySecret.tr(),
                    child: FlowyButton(
                      disable: !(state.config.enableEncrypt),
                      decoration: BoxDecoration(
                        borderRadius: Corners.s5Border,
                        border: Border.all(
                          color: Theme.of(context).colorScheme.secondary,
                        ),
                      ),
                      text: FlowyText.medium(state.config.encryptSecret),
                      onTap: () async {
                        await Clipboard.setData(
                          ClipboardData(text: state.config.encryptSecret),
                        );
                        showMessageToast(LocaleKeys.message_copy_success.tr());
                      },
                    ),
                  ),
                ),
              ],
            ),
          ],
        );
      },
    );
  }
}

class EnableSync extends StatelessWidget {
  const EnableSync({super.key});

  @override
  Widget build(BuildContext context) {
    return BlocBuilder<SupabaseCloudSettingBloc, SupabaseCloudSettingState>(
      builder: (context, state) {
        return Row(
          children: [
            FlowyText.medium(LocaleKeys.settings_menu_enableSync.tr()),
            const Spacer(),
            Switch(
              onChanged: (bool value) {
                context.read<SupabaseCloudSettingBloc>().add(
                      SupabaseCloudSettingEvent.enableSync(value),
                    );
              },
              value: state.config.enableSync,
            ),
          ],
        );
      },
    );
  }
}