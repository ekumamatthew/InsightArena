import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { User } from './entities/user.entity';
import { UsersService } from './users.service';
import { UsersController } from './users.controller';
import { Prediction } from '../predictions/entities/prediction.entity';
import { CompetitionParticipant } from 'src/competitions/entities/competition-participant.entity';

@Module({
  imports: [
    TypeOrmModule.forFeature([User, Prediction, CompetitionParticipant]),
  ],
  controllers: [UsersController],
  providers: [UsersService],
  exports: [UsersService],
})
export class UsersModule {}
